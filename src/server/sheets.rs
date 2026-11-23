//! Reading a spreadsheet into a plain grid of strings.
//!
//! No new dependency: `.xlsx` is an OOXML zip, the same shape as the `.docx`
//! files [`crate::server::rag::documents`] and [`crate::server::docs`] already
//! open with `zip` and stream with `quick_xml`. CSV is small enough to read
//! directly, and doing so keeps the delimiter sniffing and BOM handling visible
//! rather than buried in a crate's defaults.
//!
//! Everything above this layer sees only [`Sheet`]: a header row and rows of
//! cells, all already trimmed and length-capped.

use std::collections::HashMap;
use std::io::Read;

use quick_xml::events::Event;
use quick_xml::Reader;

/// The largest upload accepted, before parsing.
pub const MAX_FILE_BYTES: usize = 10 * 1024 * 1024;
/// The most data rows one import may carry.
pub const MAX_ROWS: usize = 5_000;
/// The most columns one sheet may carry.
pub const MAX_COLUMNS: usize = 100;
/// The longest single cell kept; anything past this is truncated.
pub const MAX_CELL_CHARS: usize = 500;

/// A parsed spreadsheet: one header row plus the data rows beneath it.
///
/// Every row is padded to `headers.len()`, so a consumer can index by column
/// without bounds-checking each row.
#[derive(Clone, Debug, Default)]
pub struct Sheet {
    pub sheet_name: String,
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
    /// Title rows found above the column names and left out, as their text.
    pub skipped_title_rows: Vec<String>,
}

/// How many rows at the top may be read as a title before the column names.
const MAX_TITLE_ROWS: usize = 3;

/// Cells in a row that hold something.
fn filled(row: &[String]) -> usize {
    row.iter().filter(|cell| !cell.trim().is_empty()).count()
}

/// Parse an uploaded file, choosing the reader from its extension.
pub fn parse(file_name: &str, bytes: &[u8]) -> Result<Sheet, String> {
    if bytes.is_empty() {
        return Err("That file is empty.".into());
    }
    if bytes.len() > MAX_FILE_BYTES {
        return Err(format!(
            "That file is larger than {} MB. Split it into smaller files and import them one at a time.",
            MAX_FILE_BYTES / (1024 * 1024)
        ));
    }

    let extension = file_name
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .unwrap_or_default();

    match extension.as_str() {
        "csv" | "tsv" | "txt" => parse_delimited(bytes),
        "xlsx" | "xlsm" => parse_xlsx(bytes),
        // The old binary BIFF format shares nothing with the zip-based one, and
        // every tool that writes it can also write .xlsx.
        "xls" => Err(
            "The old .xls format is not supported. Open the file and re-save it as .xlsx or .csv."
                .into(),
        ),
        "" => Err("That file has no extension, so its format cannot be determined. Use .csv or .xlsx.".into()),
        other => Err(format!(
            "Files of type .{other} are not supported. Use .csv, .tsv, or .xlsx."
        )),
    }
}

/// Trim a cell and cap its length. Import values are display strings, so an
/// over-long cell is truncated rather than failing an otherwise good row.
fn clean_cell(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.chars().count() <= MAX_CELL_CHARS {
        return trimmed.to_string();
    }
    trimmed.chars().take(MAX_CELL_CHARS).collect()
}

/// Turn a raw grid into a [`Sheet`]: first row is the header, rows are padded,
/// and fully-blank leading/trailing rows and columns are dropped.
fn finish(sheet_name: &str, mut grid: Vec<Vec<String>>) -> Result<Sheet, String> {
    grid.retain(|row| row.iter().any(|cell| !cell.trim().is_empty()));

    // Many hand-made sheets put a title such as "Outreach Tracker" in a row of
    // its own above the column names. A row with a single filled cell, followed
    // by one with several, is that kind of title rather than the header.
    // Blank rows are already gone, so every row before the first one with two
    // or more filled cells has exactly one.
    let header_at = grid
        .iter()
        .take(MAX_TITLE_ROWS + 1)
        .position(|row| filled(row) >= 2)
        .unwrap_or(0);
    let skipped_title_rows: Vec<String> = grid
        .drain(..header_at)
        .map(|title| {
            title
                .iter()
                .map(|cell| clean_cell(cell))
                .find(|cell| !cell.is_empty())
                .unwrap_or_default()
        })
        .collect();

    let mut rows = grid.into_iter();
    let Some(raw_headers) = rows.next() else {
        return Err("That file has no rows.".into());
    };

    // Trailing unnamed columns are an artefact of how spreadsheets save; a gap
    // in the middle is not, so only the tail is trimmed.
    let mut headers: Vec<String> = raw_headers.iter().map(|cell| clean_cell(cell)).collect();
    while headers.last().is_some_and(|header| header.is_empty()) {
        headers.pop();
    }
    if headers.is_empty() {
        return Err("The first row of that file is blank. It should hold the column names.".into());
    }
    if headers.len() > MAX_COLUMNS {
        return Err(format!(
            "That file has {} columns, more than the {MAX_COLUMNS} supported.",
            headers.len()
        ));
    }

    // Every column must stay addressable, so unnamed and repeated headers are
    // given a stable distinct name rather than being silently merged.
    let mut seen: HashMap<String, usize> = HashMap::new();
    for (index, header) in headers.iter_mut().enumerate() {
        if header.is_empty() {
            *header = format!("Column {}", index + 1);
        }
        let key = header.to_lowercase();
        let count = seen.entry(key).or_insert(0);
        *count += 1;
        if *count > 1 {
            *header = format!("{header} ({count})");
        }
    }

    let width = headers.len();
    let mut data = Vec::new();
    for row in rows {
        if data.len() >= MAX_ROWS {
            return Err(format!(
                "That file has more than {MAX_ROWS} rows. Split it into smaller files and import them one at a time."
            ));
        }
        let mut cells: Vec<String> = row.iter().take(width).map(|cell| clean_cell(cell)).collect();
        cells.resize(width, String::new());
        data.push(cells);
    }

    if data.is_empty() {
        return Err("That file has column names but no data rows beneath them.".into());
    }

    Ok(Sheet {
        sheet_name: sheet_name.to_string(),
        headers,
        rows: data,
        skipped_title_rows,
    })
}

// ---------------------------------------------------------------------------
// CSV / TSV
// ---------------------------------------------------------------------------

/// Pick the delimiter from the header line: whichever candidate appears most
/// often outside quotes. Beats trusting the extension, since plenty of files
/// named `.csv` are semicolon-separated exports from a European locale.
fn sniff_delimiter(text: &str) -> char {
    let header: String = text
        .chars()
        .scan(false, |in_quotes, ch| {
            if ch == '"' {
                *in_quotes = !*in_quotes;
            }
            if ch == '\n' && !*in_quotes {
                return None;
            }
            Some((ch, *in_quotes))
        })
        .filter(|(_, in_quotes)| !in_quotes)
        .map(|(ch, _)| ch)
        .collect();

    [',', '\t', ';', '|']
        .into_iter()
        .max_by_key(|candidate| header.matches(*candidate).count())
        .filter(|candidate| header.contains(*candidate))
        .unwrap_or(',')
}

/// RFC 4180 with the usual real-world tolerances: a UTF-8 BOM, `\r\n` or `\n`
/// line endings, quoted fields holding the delimiter or newlines, and `""` for a
/// literal quote.
fn parse_delimited(bytes: &[u8]) -> Result<Sheet, String> {
    // Lossy so one bad byte in one cell cannot fail a 4000-row import.
    let text = String::from_utf8_lossy(bytes);
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let delimiter = sniff_delimiter(text);

    let mut grid: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if in_quotes {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    in_quotes = false;
                }
            } else {
                field.push(ch);
            }
            continue;
        }
        match ch {
            // A quote only opens a field at its start; mid-field it is literal,
            // which is what a stray inch mark in an address amounts to.
            '"' if field.is_empty() => in_quotes = true,
            c if c == delimiter => row.push(std::mem::take(&mut field)),
            '\r' => {}
            '\n' => {
                row.push(std::mem::take(&mut field));
                grid.push(std::mem::take(&mut row));
                // Bail early on a runaway file rather than buffering all of it.
                if grid.len() > MAX_ROWS + 1 {
                    return Err(format!(
                        "That file has more than {MAX_ROWS} rows. Split it into smaller files and import them one at a time."
                    ));
                }
            }
            other => field.push(other),
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        grid.push(row);
    }

    finish("", grid)
}

// ---------------------------------------------------------------------------
// XLSX
// ---------------------------------------------------------------------------

/// Read one entry out of the zip as a string, if it exists.
fn read_entry<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    name: &str,
) -> Option<String> {
    let mut entry = archive.by_name(name).ok()?;
    let mut text = String::new();
    entry.read_to_string(&mut text).ok()?;
    Some(text)
}

/// The value of an attribute on a start tag, ignoring any namespace prefix.
fn attribute(tag: &quick_xml::events::BytesStart, want: &[u8]) -> Option<String> {
    tag.attributes().flatten().find_map(|attr| {
        let key = attr.key.as_ref();
        let local = key.rsplit(|b| *b == b':').next().unwrap_or(key);
        (local == want).then(|| String::from_utf8_lossy(&attr.value).into_owned())
    })
}

fn local_name(name: &[u8]) -> &[u8] {
    name.rsplit(|b| *b == b':').next().unwrap_or(name)
}

/// `"BC7"` -> `54`. Column letters are base-26 with no zero.
fn column_index(reference: &str) -> Option<usize> {
    let letters: String = reference
        .chars()
        .take_while(|c| c.is_ascii_alphabetic())
        .collect();
    if letters.is_empty() {
        return None;
    }
    let mut index = 0usize;
    for ch in letters.chars() {
        index = index.checked_mul(26)?;
        index = index.checked_add((ch.to_ascii_uppercase() as u8 - b'A' + 1) as usize)?;
    }
    Some(index - 1)
}

/// The shared string table: `xl/sharedStrings.xml` holds every distinct string
/// in the workbook once, and cells with `t="s"` carry an index into it.
fn parse_shared_strings(xml: &str) -> Vec<String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut strings = Vec::new();
    let mut current = String::new();
    let mut in_item = false;
    let mut in_text = false;
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Err(_) | Ok(Event::Eof) => break,
            Ok(Event::Start(tag)) => match local_name(tag.name().as_ref()) {
                b"si" => {
                    in_item = true;
                    current.clear();
                }
                // A single `<si>` may hold several `<t>` runs when part of the
                // string is styled differently; they concatenate.
                b"t" if in_item => in_text = true,
                _ => {}
            },
            Ok(Event::End(tag)) => match local_name(tag.name().as_ref()) {
                b"si" => {
                    in_item = false;
                    strings.push(std::mem::take(&mut current));
                }
                b"t" => in_text = false,
                _ => {}
            },
            Ok(Event::Text(text)) if in_text => {
                current.push_str(&text.unescape().unwrap_or_default());
            }
            _ => {}
        }
        buf.clear();
    }
    strings
}

/// The first sheet's name and its relationship id, from `xl/workbook.xml`.
fn first_sheet(xml: &str) -> Option<(String, String)> {
    let mut reader = Reader::from_str(xml);
    let mut buf = Vec::new();
    loop {
        let event = reader.read_event_into(&mut buf);
        let tag = match &event {
            Ok(Event::Start(tag)) | Ok(Event::Empty(tag)) => tag,
            Err(_) | Ok(Event::Eof) => return None,
            _ => {
                buf.clear();
                continue;
            }
        };
        if local_name(tag.name().as_ref()) == b"sheet" {
            let name = attribute(tag, b"name").unwrap_or_default();
            let rel = attribute(tag, b"id").unwrap_or_default();
            return Some((name, rel));
        }
        buf.clear();
    }
}

/// Resolve a relationship id to its target path via `xl/_rels/workbook.xml.rels`.
fn resolve_relationship(xml: &str, rel_id: &str) -> Option<String> {
    let mut reader = Reader::from_str(xml);
    let mut buf = Vec::new();
    loop {
        let event = reader.read_event_into(&mut buf);
        let tag = match &event {
            Ok(Event::Start(tag)) | Ok(Event::Empty(tag)) => tag,
            Err(_) | Ok(Event::Eof) => return None,
            _ => {
                buf.clear();
                continue;
            }
        };
        if local_name(tag.name().as_ref()) == b"Relationship"
            && attribute(tag, b"Id").as_deref() == Some(rel_id)
        {
            let target = attribute(tag, b"Target")?;
            let target = target.trim_start_matches('/');
            return Some(if target.starts_with("xl/") {
                target.to_string()
            } else {
                format!("xl/{target}")
            });
        }
        buf.clear();
    }
}

/// Stream one worksheet into a grid.
///
/// Cells are placed by their `r="B7"` reference rather than by arrival order: a
/// blank cell is simply absent from the XML, so counting `<c>` elements would
/// silently shift every value after a gap one column to the left.
fn parse_worksheet(xml: &str, shared: &[String]) -> Vec<Vec<String>> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);

    let mut grid: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut cell_index = 0usize;
    let mut cell_type = String::new();
    let mut value = String::new();
    let mut in_value = false;
    let mut in_inline_text = false;
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Err(_) | Ok(Event::Eof) => break,
            Ok(Event::Start(tag)) => match local_name(tag.name().as_ref()) {
                b"row" => row = Vec::new(),
                b"c" => {
                    cell_index = attribute(&tag, b"r")
                        .as_deref()
                        .and_then(column_index)
                        .unwrap_or(row.len());
                    cell_type = attribute(&tag, b"t").unwrap_or_default();
                    value.clear();
                }
                b"v" => in_value = true,
                b"t" => in_inline_text = true,
                _ => {}
            },
            // A self-closing `<row/>` is an empty row: it still has to be kept
            // so the rows after it do not shift up. A self-closing `<c/>` is a
            // styled-but-blank cell, which `finish` pads back in anyway.
            Ok(Event::Empty(tag)) => {
                if local_name(tag.name().as_ref()) == b"row" {
                    grid.push(Vec::new());
                }
            }
            Ok(Event::End(tag)) => match local_name(tag.name().as_ref()) {
                b"row" => grid.push(std::mem::take(&mut row)),
                b"c" => {
                    // `t="s"` is an index into the shared table; everything else
                    // (numbers, inline strings, booleans, dates) is already text.
                    let text = if cell_type == "s" {
                        value
                            .trim()
                            .parse::<usize>()
                            .ok()
                            .and_then(|index| shared.get(index).cloned())
                            .unwrap_or_default()
                    } else {
                        value.clone()
                    };
                    if row.len() <= cell_index {
                        row.resize(cell_index + 1, String::new());
                    }
                    row[cell_index] = text;
                    value.clear();
                    cell_type.clear();
                }
                b"v" => in_value = false,
                b"t" => in_inline_text = false,
                _ => {}
            },
            Ok(Event::Text(text)) if in_value || in_inline_text => {
                value.push_str(&text.unescape().unwrap_or_default());
            }
            _ => {}
        }
        buf.clear();
    }
    grid
}

fn parse_xlsx(bytes: &[u8]) -> Result<Sheet, String> {
    let cursor = std::io::Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(cursor)
        .map_err(|_| "That file is not a readable .xlsx workbook.".to_string())?;

    let workbook = read_entry(&mut archive, "xl/workbook.xml")
        .ok_or_else(|| "That .xlsx file has no workbook inside it.".to_string())?;
    let (sheet_name, rel_id) = first_sheet(&workbook)
        .ok_or_else(|| "That workbook has no sheets.".to_string())?;

    // Prefer the declared relationship; fall back to the conventional path, as
    // some generators omit the rels entry.
    let path = read_entry(&mut archive, "xl/_rels/workbook.xml.rels")
        .and_then(|rels| resolve_relationship(&rels, &rel_id))
        .filter(|path| archive.by_name(path).is_ok())
        .unwrap_or_else(|| "xl/worksheets/sheet1.xml".to_string());

    let worksheet = read_entry(&mut archive, &path)
        .ok_or_else(|| "The first sheet of that workbook could not be read.".to_string())?;
    let shared = read_entry(&mut archive, "xl/sharedStrings.xml")
        .map(|xml| parse_shared_strings(&xml))
        .unwrap_or_default();

    finish(&sheet_name, parse_worksheet(&worksheet, &shared))
}
