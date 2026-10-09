//! Text-layer PDF extraction shared by selected attachments and Read results.
use crate::{Error, Result};

pub(crate) fn pdf_text(data: &[u8]) -> Result<String> {
    if data.len() > 32 * 1024 * 1024 {
        return Err(Error::Protocol(
            "PDF exceeds 32 MiB extraction limit".into(),
        ));
    }
    let document = lopdf::Document::load_mem(data)
        .map_err(|error| Error::Protocol(format!("cannot parse PDF: {error}")))?;
    if document.is_encrypted() {
        return Err(Error::Protocol("PDF requires a password".into()));
    }
    let pages = document.get_pages();
    if pages.len() > 200 {
        return Err(Error::Protocol(
            "PDF exceeds 200-page extraction limit".into(),
        ));
    }
    let mut output = String::new();
    for page in pages.keys() {
        let text = document
            .extract_text(&[*page])
            .map_err(|error| Error::Protocol(format!("cannot extract PDF page {page}: {error}")))?;
        if output.len() + text.len() > 2 * 1024 * 1024 {
            return Err(Error::Protocol(
                "PDF text exceeds 2 MiB extraction limit".into(),
            ));
        }
        output.push_str(&format!("\n<page number=\"{page}\">\n{text}\n</page>\n"));
    }
    Ok(format!(
        "PDF text layer (images, diagrams and scanned text are not extracted):\n{output}"
    ))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use lopdf::{dictionary, Document, Object, Stream};

    pub(crate) fn compressed_pdf() -> Vec<u8> {
        let mut document = Document::with_version("1.5");
        let pages = document.new_object_id();
        let font = document.add_object(dictionary! { "Type" => "Font", "Subtype" => "Type1", "BaseFont" => "Helvetica", "Encoding" => "WinAnsiEncoding" });
        let resources = document.add_object(dictionary! { "Font" => dictionary! { "F1" => font } });
        let mut children = Vec::new();
        for text in ["Compressed page one: amber-942", "Second page: violet-315"] {
            let content = document.add_object(Stream::new(
                dictionary! {},
                format!("BT /F1 16 Tf 50 750 Td ({}) Tj ET", text.repeat(10)).into_bytes(),
            ));
            let page = document.add_object(dictionary! { "Type" => "Page", "Parent" => pages, "Resources" => resources, "Contents" => content, "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()] });
            children.push(Object::Reference(page));
        }
        document.objects.insert(
            pages,
            dictionary! { "Type" => "Pages", "Kids" => children, "Count" => 2 }.into(),
        );
        let catalog = document.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
        document.trailer.set("Root", catalog);
        document.compress();
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).unwrap();
        bytes
    }

    #[test]
    fn extracts_compressed_pages_and_rejects_broken_pdf() {
        let bytes = compressed_pdf();
        assert!(bytes
            .windows(b"FlateDecode".len())
            .any(|window| window == b"FlateDecode"));
        let text = pdf_text(&bytes).unwrap();
        assert!(text.contains("amber-942"));
        assert!(text.contains("violet-315"));
        assert!(text.find("amber-942").unwrap() < text.find("violet-315").unwrap());
        assert!(text.contains("<page number=\"2\">"));
        assert!(pdf_text(b"%PDF-broken").is_err());
    }
}
