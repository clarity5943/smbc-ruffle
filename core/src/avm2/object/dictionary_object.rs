//! Object representation for `flash.utils.Dictionary`

use crate::avm2::Error;
use crate::avm2::activation::Activation;
use crate::avm2::dynamic_map::DynamicKey;
use crate::avm2::object::script_object::ScriptObjectData;
use crate::avm2::object::{ClassObject, Object, TObject};
use crate::avm2::value::Value;
use crate::string::AvmString;
use core::fmt;
use crate::avm2::object::WeakObject;
use gc_arena::barrier::unlock;
use gc_arena::lock::RefLock;
use gc_arena::{Collect, Gc, GcWeak, Mutation};
use std::cell::Cell;
use ruffle_common::utils::HasPrefixField;

/// A class instance allocator that allocates Dictionary objects.
pub fn dictionary_allocator<'gc>(
    class: ClassObject<'gc>,
    activation: &mut Activation<'_, 'gc>,
) -> Result<Object<'gc>, Error<'gc>> {
    let base = ScriptObjectData::new(class);

    Ok(DictionaryObject(Gc::new(
        activation.gc(),
        DictionaryObjectData {
            base,
            weak_keys: Cell::new(false),
            weak: RefLock::new(WeakTable::default()),
        },
    ))
    .into())
}

/// An object that allows associations between objects and values.
///
/// This is implemented by way of "object space", parallel to the property
/// space that ordinary properties live in. This space has no namespaces, and
/// keys are objects instead of strings.
#[derive(Clone, Collect, Copy)]
#[collect(no_drop)]
pub struct DictionaryObject<'gc>(pub Gc<'gc, DictionaryObjectData<'gc>>);

#[derive(Clone, Collect, Copy, Debug)]
#[collect(no_drop)]
pub struct DictionaryObjectWeak<'gc>(pub GcWeak<'gc, DictionaryObjectData<'gc>>);

impl fmt::Debug for DictionaryObject<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DictionaryObject")
            .field("ptr", &Gc::as_ptr(self.0))
            .finish()
    }
}

#[derive(Clone, Collect, HasPrefixField)]
#[collect(no_drop)]
#[repr(C, align(8))]
pub struct DictionaryObjectData<'gc> {
    /// Base script object
    base: ScriptObjectData<'gc>,

    /// `new Dictionary(true)`: object keys are held weakly (in `weak`), so
    /// entries disappear once nothing else references their key. Without this,
    /// SMBC's weak-keyed caches keep every old level alive and the game slows
    /// down the longer it runs.
    weak_keys: Cell<bool>,
    weak: RefLock<WeakTable<'gc>>,
}

/// Object-keyed entries of a weak-keyed dictionary. Removed entries leave a
/// hole so that enumeration indices stay valid; holes and entries whose key
/// was collected are dropped by `compact`.
#[derive(Clone, Collect, Default)]
#[collect(no_drop)]
struct WeakTable<'gc> {
    entries: Vec<Option<(WeakObject<'gc>, Value<'gc>)>>,
    #[collect(require_static)]
    index: fnv::FnvHashMap<usize, usize>,
    #[collect(require_static)]
    live: usize,
    #[collect(require_static)]
    compact_at: usize,
}

/// Enumeration indices of weak entries start here.
const WEAK_ENUM_BASE: u32 = 1 << 30;

impl<'gc> WeakTable<'gc> {
    fn key(obj: Object<'gc>) -> usize {
        obj.as_ptr() as usize
    }

    fn get(&self, obj: Object<'gc>) -> Option<Value<'gc>> {
        let i = *self.index.get(&Self::key(obj))?;
        self.entries[i].map(|(_, v)| v)
    }

    fn set(&mut self, obj: Object<'gc>, value: Value<'gc>, mc: &Mutation<'gc>) {
        let k = Self::key(obj);
        if let Some(&i) = self.index.get(&k)
            && let Some(e) = self.entries[i].as_mut()
        {
            e.1 = value;
            return;
        }
        if self.entries.len() >= self.compact_at {
            self.compact(mc);
        }
        self.index.insert(k, self.entries.len());
        self.entries.push(Some((obj.downgrade(), value)));
        self.live += 1;
    }

    fn remove(&mut self, obj: Object<'gc>) {
        if let Some(i) = self.index.remove(&Self::key(obj))
            && self.entries[i].take().is_some()
        {
            self.live -= 1;
        }
    }

    /// Drops holes and entries whose key has been garbage collected.
    fn compact(&mut self, mc: &Mutation<'gc>) {
        let old = std::mem::take(&mut self.entries);
        self.index.clear();
        for (k, v) in old.into_iter().flatten() {
            if k.upgrade(mc).is_some() {
                self.index.insert(k.as_ptr() as usize, self.entries.len());
                self.entries.push(Some((k, v)));
            }
        }
        self.live = self.entries.len();
        self.compact_at = (self.entries.len() * 2).max(64);
    }

    /// Next occupied slot at or after `from` whose key is still alive.
    fn next_live(&self, from: usize, mc: &Mutation<'gc>) -> Option<usize> {
        (from..self.entries.len())
            .find(|&i| self.entries[i].is_some_and(|(k, _)| k.upgrade(mc).is_some()))
    }
}

impl<'gc> DictionaryObject<'gc> {
    /// Switch to weak object keys (`new Dictionary(true)`). Only takes effect
    /// while the dictionary has no object keys yet.
    pub fn set_weak_keys(self, weak: bool) {
        self.0.weak_keys.set(weak);
    }

    fn weak_mut(self, mc: &Mutation<'gc>) -> std::cell::RefMut<'gc, WeakTable<'gc>> {
        unlock!(Gc::write(mc, self.0), DictionaryObjectData, weak).borrow_mut()
    }

    /// Retrieve a value in the dictionary's object space.
    pub fn get_property_by_object(self, name: Object<'gc>) -> Value<'gc> {
        if self.0.weak_keys.get() {
            return self.0.weak.borrow().get(name).unwrap_or(Value::Undefined);
        }
        self.base()
            .values()
            .get(&DynamicKey::Object(name))
            .map(|v| v.value)
            .unwrap_or(Value::Undefined)
    }

    /// Set a value in the dictionary's object space.
    pub fn set_property_by_object(self, name: Object<'gc>, value: Value<'gc>, mc: &Mutation<'gc>) {
        if self.0.weak_keys.get() {
            self.weak_mut(mc).set(name, value, mc);
            return;
        }
        self.base()
            .values_mut(mc)
            .insert(DynamicKey::Object(name), value);
    }

    /// Delete a value from the dictionary's object space.
    pub fn delete_property_by_object(self, name: Object<'gc>, mc: &Mutation<'gc>) {
        if self.0.weak_keys.get() {
            self.weak_mut(mc).remove(name);
            return;
        }
        self.base().values_mut(mc).remove(&DynamicKey::Object(name));
    }

    pub fn has_property_by_object(self, name: Object<'gc>) -> bool {
        if self.0.weak_keys.get() {
            return self.0.weak.borrow().get(name).is_some();
        }
        self.base().values().contains_key(&DynamicKey::Object(name))
    }
}

impl<'gc> TObject<'gc> for DictionaryObject<'gc> {
    fn gc_base(&self) -> Gc<'gc, ScriptObjectData<'gc>> {
        HasPrefixField::as_prefix_gc(self.0)
    }

    // Calling `setPropertyIsEnumerable` on a `Dictionary` has no effect -
    // stringified properties are always enumerable.
    fn set_local_property_is_enumerable(
        &self,
        _mc: &Mutation<'gc>,
        _name: AvmString<'gc>,
        _is_enumerable: bool,
    ) {
    }

    fn get_next_enumerant(
        self,
        last_index: u32,
        activation: &mut Activation<'_, 'gc>,
    ) -> Result<u32, Error<'gc>> {
        if !self.0.weak_keys.get() {
            return Ok(self.base().get_next_enumerant(last_index));
        }
        if last_index < WEAK_ENUM_BASE {
            let n = self.base().get_next_enumerant(last_index);
            if n != 0 {
                return Ok(n);
            }
        }
        let from = last_index.saturating_sub(WEAK_ENUM_BASE) as usize;
        Ok(self
            .0
            .weak
            .borrow()
            .next_live(from, activation.gc())
            .map(|i| WEAK_ENUM_BASE + i as u32 + 1)
            .unwrap_or(0))
    }

    fn get_enumerant_name(
        self,
        index: u32,
        activation: &mut Activation<'_, 'gc>,
    ) -> Result<Value<'gc>, Error<'gc>> {
        if self.0.weak_keys.get() && index > WEAK_ENUM_BASE {
            let i = (index - WEAK_ENUM_BASE - 1) as usize;
            let w = self.0.weak.borrow();
            return Ok(w
                .entries
                .get(i)
                .copied()
                .flatten()
                .and_then(|(k, _)| k.upgrade(activation.gc()))
                .map(Value::Object)
                .unwrap_or(Value::Null));
        }
        Ok(self.base().get_enumerant_name(index).unwrap_or(Value::Null))
    }

    fn get_enumerant_value(
        self,
        index: u32,
        _activation: &mut Activation<'_, 'gc>,
    ) -> Result<Value<'gc>, Error<'gc>> {
        if self.0.weak_keys.get() && index > WEAK_ENUM_BASE {
            let i = (index - WEAK_ENUM_BASE - 1) as usize;
            let w = self.0.weak.borrow();
            return Ok(w
                .entries
                .get(i)
                .copied()
                .flatten()
                .map(|(_, v)| v)
                .unwrap_or(Value::Undefined));
        }
        Ok(*self
            .base()
            .values()
            .value_at(index as usize)
            .unwrap_or(&Value::Undefined))
    }
}
