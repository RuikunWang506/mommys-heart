//! A minimal `.xlsx` writer (SSR only): one worksheet of text cells with a bold,
//! frozen header row.
//!
//! No new dependency: like the `.docx` case-note records, an `.xlsx` file is an
//! OOXML zip, written here with `zip`. Every cell is an inline string, so a
//! value such as `=SUM(A1)` is shown as text and never run as a formula.

use std::io::{Cursor, Write};

use zip::write::SimpleFileOptions;

/// Excel's limit on the characters in one cell.
const MAX_CELL_CHARS: usize = 32_767;

const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/>
<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>
<Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>
<Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/>
</Types>"#;

const PACKAGE_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>
</Relationships>"#;

const WORKBOOK_RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>
<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>
</Relationships>"#;

/// Two cell formats: 0 is plain, 1 is bold (the header row).
const STYLES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
<fonts count="2"><font><sz val="11"/><name val="Calibri"/></font><font><b/><sz val="11"/><name val="Calibri"/></font></fonts>
<fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills>
<borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders>
<cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs>
<cellXfs count="2"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/><xf numFmtId="0" fontId="1" fillId="0" borderId="0" xfId="0" applyFont="1"/></cellXfs>
<cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles>
</styleSheet>"#;

/// One column: its header text and width in Excel's character units.
pub struct Column<'a> {
    pub header: &'a str,
    pub width: f64,
}

/// Build a workbook with one sheet: a bold, frozen header row, then `rows`.
/// A row shorter than `columns` leaves the rest blank; extra values are dropped.
pub fn workbook(
    sheet_name: &str,
    columns: &[Column<'_>],
    rows: &[Vec<String>],
) -> Result<Vec<u8>, String> {
    let sheet = worksheet_xml(columns, rows);
    let workbook = format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">
<sheets><sheet name="{}" sheetId="1" r:id="rId1"/></sheets>
</workbook>"#,
        escape(&sheet_title(sheet_name))
    );

    let mut buffer = Cursor::new(Vec::new());
    {
        let mut zip = zip::ZipWriter::new(&mut buffer);
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        // `[Content_Types].xml` first, as content sniffers expect.
        for (name, contents) in [
            ("[Content_Types].xml", CONTENT_TYPES),
            ("_rels/.rels", PACKAGE_RELS),
            ("xl/workbook.xml", workbook.as_str()),
            ("xl/_rels/workbook.xml.rels", WORKBOOK_RELS),
            ("xl/styles.xml", STYLES),
            ("xl/worksheets/sheet1.xml", sheet.as_str()),
        ] {
            zip.start_file(name, options)
                .map_err(|e| format!("could not start '{name}' in the workbook: {e}"))?;
            zip.write_all(contents.as_bytes())
                .map_err(|e| format!("could not write '{name}' in the workbook: {e}"))?;
        }
        zip.finish()
            .map_err(|e| format!("could not finish the workbook: {e}"))?;
    }
    Ok(buffer.into_inner())
}

fn worksheet_xml(columns: &[Column<'_>], rows: &[Vec<String>]) -> String {
    let mut xml = String::from(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">
<sheetViews><sheetView workbookViewId="0"><pane ySplit="1" topLeftCell="A2" activePane="bottomLeft" state="frozen"/></sheetView></sheetViews>
<cols>"#,
    );
    for (i, column) in columns.iter().enumerate() {
        xml.push_str(&format!(
            r#"<col min="{n}" max="{n}" width="{w}" customWidth="1"/>"#,
            n = i + 1,
            w = column.width
        ));
    }
    xml.push_str("</cols><sheetData>");

    let header: Vec<String> = columns.iter().map(|c| c.header.to_string()).collect();
    push_row(&mut xml, 1, &header, columns.len(), 1);
    for (i, row) in rows.iter().enumerate() {
        push_row(&mut xml, i + 2, row, columns.len(), 0);
    }
    xml.push_str("</sheetData>");
    if !columns.is_empty() {
        xml.push_str(&format!(
            r#"<autoFilter ref="A1:{}{}"/>"#,
            column_letter(columns.len() - 1),
            rows.len() + 1
        ));
    }
    xml.push_str("</worksheet>");
    xml
}

fn push_row(xml: &mut String, number: usize, values: &[String], width: usize, style: u8) {
    xml.push_str(&format!(r#"<row r="{number}">"#));
    for (i, value) in values.iter().take(width).enumerate() {
        if value.is_empty() {
            continue;
        }
        let text: String = value.chars().take(MAX_CELL_CHARS).collect();
        let style_attr = if style == 0 {
            String::new()
        } else {
            format!(r#" s="{style}""#)
        };
        xml.push_str(&format!(
            r#"<c r="{col}{number}" t="inlineStr"{style_attr}><is><t xml:space="preserve">{text}</t></is></c>"#,
            col = column_letter(i),
            text = escape(&text)
        ));
    }
    xml.push_str("</row>");
}

/// `0` -> `A`, `25` -> `Z`, `26` -> `AA`.
fn column_letter(mut index: usize) -> String {
    let mut letters = Vec::new();
    loop {
        letters.push((b'A' + (index % 26) as u8) as char);
        if index < 26 {
            break;
        }
        index = index / 26 - 1;
    }
    letters.iter().rev().collect()
}

/// Excel sheet names: at most 31 characters, none of `[]:*?/\`.
fn sheet_title(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .filter(|c| !matches!(c, '[' | ']' | ':' | '*' | '?' | '/' | '\\'))
        .take(31)
        .collect();
    if cleaned.trim().is_empty() {
        "Sheet1".to_string()
    } else {
        cleaned
    }
}

/// XML-escape text and drop the control characters XML 1.0 cannot carry
/// (keeping tab and newline, which a cell can show).
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            '\t' | '\n' => out.push(c),
            '\r' => {}
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}
