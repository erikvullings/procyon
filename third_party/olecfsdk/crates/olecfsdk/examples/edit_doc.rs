use std::{env, io, path::PathBuf};

use olecfsdk::doc::{DocFile, TextPieceCharacters};

fn main() -> Result<(), Box<dyn std::error::Error>> {
  let (input, output) = paths("edit_doc <input.doc> <output.doc>")?;
  let mut file = DocFile::open(input)?;

  let main_length = u32::try_from(file.word_document.fib.rg_lw.ccp_text)?;
  let (cp, replacement) = file
    .word_document
    .text_pieces
    .iter()
    .find_map(|piece| {
      let start = u32::try_from(piece.value.cp_start).ok()?;
      if start >= main_length {
        return None;
      }
      let TextPieceCharacters::String(value) = &piece.value.characters else {
        return None;
      };
      let first = value.value.chars().next()?;
      if first.len_utf16() != 1 {
        return None;
      }
      let edited = if first == 'X' { "Y" } else { "X" };
      Some((start, edited.to_owned()))
    })
    .ok_or_else(|| io::Error::other("DOC has no editable main-text character"))?;

  file.replace_main_text_range(cp..cp + 1, replacement)?;
  file.save(&output)?;
  let reopened = DocFile::open(&output)?;
  println!(
    "saved {} DOC parts and {} text pieces to {}",
    reopened.content_tree()?.parts().len(),
    reopened.word_document.text_pieces.len(),
    output.display()
  );
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
