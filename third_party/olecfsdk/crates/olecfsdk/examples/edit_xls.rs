use std::{env, io, path::PathBuf};

use olecfsdk::xls::XlsFile;

fn main() -> Result<(), Box<dyn std::error::Error>> {
  let (input, output) = paths("edit_xls <input.xls> <output.xls>")?;
  let mut file = XlsFile::open(input)?;

  let (workbook_name, sheet_id, mut edited_name) = file
    .workbooks
    .iter()
    .find_map(|workbook| {
      let relationships = workbook.relationships().ok()?;
      let sheet = relationships.sheets().first()?;
      Some((workbook.name, sheet.id(), sheet.metadata().name.clone()))
    })
    .ok_or_else(|| io::Error::other("XLS has no editable worksheet"))?;
  if edited_name.value.encode_utf16().count() < 31 {
    edited_name.value.push('X');
  } else {
    let first_character_bytes = edited_name
      .value
      .chars()
      .next()
      .expect("sheet names are nonempty")
      .len_utf8();
    edited_name
      .value
      .replace_range(..first_character_bytes, "X");
  }

  file.set_sheet_name(workbook_name, sheet_id, edited_name)?;
  file.save(&output)?;
  let reopened = XlsFile::open(&output)?;
  let sheet_count = reopened
    .workbooks
    .iter()
    .map(|workbook| {
      workbook
        .relationships()
        .map(|relationships| relationships.sheets().len())
    })
    .collect::<olecfsdk::Result<Vec<_>>>()?
    .into_iter()
    .sum::<usize>();
  println!(
    "saved {} workbook stream(s) and {sheet_count} sheet(s) to {}",
    reopened.workbooks.len(),
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
