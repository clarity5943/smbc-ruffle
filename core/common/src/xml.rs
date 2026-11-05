use quick_xml::escape::EscapeError as XmlEscapeError;
use std::str::Utf8Error;

/// How entity candidates are delimited (equivalent to the regexes
/// `&[^;&]*;` for AVM1 and `&[^;]*;` for AVM2, but found with a linear scan:
/// the regex engine backtracks quadratically on long text containing many
/// `&` characters without a `;`, e.g. URL-encoded level data).
#[derive(Clone, Copy, PartialEq)]
enum EntityMode {
    /// `&[^;&]*;` — an entity may not contain another '&'.
    Avm1,
    /// `&[^;]*;` — everything from an '&' up to the next ';'.
    Avm2,
}

/// Finds the next entity candidate at or after `from`, returning `(start, end)`.
fn next_entity(bytes: &[u8], mut from: usize, mode: EntityMode) -> Option<(usize, usize)> {
    loop {
        let start = from + memchr_byte(b'&', &bytes[from..])?;
        let mut i = start + 1;
        loop {
            match bytes.get(i) {
                None => return None, // no ';' anywhere after: no more matches
                Some(b';') => return Some((start, i + 1)),
                Some(b'&') if mode == EntityMode::Avm1 => break, // retry from this '&'
                _ => i += 1,
            }
        }
        from = i;
    }
}

fn memchr_byte(needle: u8, haystack: &[u8]) -> Option<usize> {
    haystack.iter().position(|&b| b == needle)
}

/// AVM1 XML unescaping. Decodes entities individually, even when
/// preceded by a bare '&' (e.g. "&&amp;" becomes "&&").
pub fn avm1_unescape(input: &[u8]) -> Result<String, Utf8Error> {
    custom_unescape(input, EntityMode::Avm1)
}

/// AVM2 E4X XML unescaping. Does not decode entities preceded by a
/// bare '&' (e.g. "&&amp;" is preserved as "&&amp;").
pub fn avm2_unescape(input: &[u8]) -> Result<String, Utf8Error> {
    custom_unescape(input, EntityMode::Avm2)
}

/// Handles flash-specific XML unescaping behavior.
/// We accept all XML entities, and also accept standalone '&' without
/// a corresponding ';'
fn custom_unescape(input: &[u8], mode: EntityMode) -> Result<String, Utf8Error> {
    let input = std::str::from_utf8(input)?;
    if !input.as_bytes().contains(&b'&') {
        return Ok(input.to_owned());
    }

    let mut result = String::new();
    let mut last_end = 0;

    // Find all entities, and try to unescape them.
    // Our regular expression will skip over '&' without a matching ';',
    // which will preserve them as-is in the output
    let bytes = input.as_bytes();
    while let Some((start, end)) = next_entity(bytes, last_end, mode) {

        result.push_str(&input[last_end..start]);

        let entity = &input[start..end];
        // Unfortunately, we need to call this on each entity individually,
        // since it bails out if *any* entities in the string lack a terminating ';'
        match quick_xml::escape::unescape(entity) {
            Ok(decoded) => result.push_str(&decoded),
            // Unknown or malformed entities are preserved as-is.
            // The AVM2 regex can match strings like "&&amp;" which contain
            // multiple '&' characters, causing unescape to report UnterminatedEntity
            // for the first '&' before it reaches the ';'.
            Err(
                XmlEscapeError::InvalidCharRef(_)
                | XmlEscapeError::UnrecognizedEntity(_, _)
                | XmlEscapeError::UnterminatedEntity(_),
            ) => result.push_str(entity),
        }

        last_end = end;
    }

    result.push_str(&input[last_end..]);

    Ok(result)
}
