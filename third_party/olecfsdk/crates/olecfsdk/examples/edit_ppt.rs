use std::{env, io, path::PathBuf};

use olecfsdk::ppt::{PptFile, PptRecordData};

fn main() -> Result<(), Box<dyn std::error::Error>> {
  let (input, output) = paths("edit_ppt <input.ppt> <output.ppt>")?;
  let mut file = PptFile::open(input)?;

  let (slide_id, body_index, unicode, replacement) = {
    let presentation = file.live_presentation()?;
    presentation
      .slides()?
      .into_iter()
      .find_map(|slide| {
        slide
          .object
          .text_bodies()
          .iter()
          .enumerate()
          .find_map(|(body_index, body)| {
            body.records.iter().find_map(|record| match &record.data {
              PptRecordData::TextChars(characters) if !characters.is_empty() => {
                let replacement = if characters.starts_with('X') {
                  'Y'
                } else {
                  'X'
                };
                Some((slide.id(), body_index, true, replacement))
              }
              PptRecordData::TextBytes(characters) if !characters.is_empty() => {
                let replacement = if characters.starts_with('X') {
                  'Y'
                } else {
                  'X'
                };
                Some((slide.id(), body_index, false, replacement))
              }
              _ => None,
            })
          })
      })
      .ok_or_else(|| io::Error::other("PPT has no editable slide text"))?
  };

  file.edit_slide_text_body(slide_id, body_index, |mut body| {
    for record in body.records_mut() {
      match (&mut record.data, unicode) {
        (PptRecordData::TextChars(characters), true) if !characters.is_empty() => {
          let end = characters
            .chars()
            .next()
            .expect("nonempty PPT String")
            .len_utf8();
          characters.replace_range(..end, &replacement.to_string());
          return Ok(());
        }
        (PptRecordData::TextBytes(characters), false) if !characters.is_empty() => {
          let end = characters
            .chars()
            .next()
            .expect("nonempty PPT String")
            .len_utf8();
          characters.replace_range(..end, &replacement.to_string());
          return Ok(());
        }
        _ => {}
      }
    }
    Err(olecfsdk::Error::invalid(
      0,
      "selected PPT body changed text representation",
    ))
  })?;

  file.save(&output)?;
  let reopened = PptFile::open(&output)?;
  let slide_count = reopened.live_presentation()?.slides()?.len();
  println!("saved {slide_count} slide(s) to {}", output.display());
  Ok(())
}

fn paths(usage: &str) -> Result<(PathBuf, PathBuf), io::Error> {
  let mut arguments = env::args_os().skip(1);
  let input = arguments
    .next()
    .map(PathBuf::from)
    .ok_or_else(|| io::Error::other(usage))?;
  let output = arguments
    .next()
    .map(PathBuf::from)
    .ok_or_else(|| io::Error::other(usage))?;
  if arguments.next().is_some() {
    return Err(io::Error::other(usage));
  }
  Ok((input, output))
}
