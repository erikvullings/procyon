use olecfsdk::{
  property_set::Property,
  shared_content::{OfficePropertySetData, OfficePropertySetKind, OfficeSharedContent},
};
use ooxmlsdk::{
  common::XmlNamespace, namespaces::XmlKnownNamespace, schemas::opc_core_properties as cp,
};

use crate::{
  ConversionCode, ConversionOptions, ConversionReport, Disposition, Error, LossPolicy, Result,
  SourceLocation,
};

const PID_CODE_PAGE: u32 = 1;

const PIDSI_TITLE: u32 = 2;
const PIDSI_SUBJECT: u32 = 3;
const PIDSI_AUTHOR: u32 = 4;
const PIDSI_KEYWORDS: u32 = 5;
const PIDSI_COMMENTS: u32 = 6;
const PIDSI_LAST_AUTHOR: u32 = 8;
const PIDSI_REVISION_NUMBER: u32 = 9;

const PIDDSI_CATEGORY: u32 = 2;
const PIDDSI_CONTENT_TYPE: u32 = 0x1a;
const PIDDSI_CONTENT_STATUS: u32 = 0x1b;
const PIDDSI_LANGUAGE: u32 = 0x1c;

pub(crate) fn convert_core_properties(
  source: &OfficeSharedContent,
  options: ConversionOptions,
  report: &mut ConversionReport,
) -> Result<Option<cp::CoreProperties>> {
  let mut target = cp::CoreProperties::default();
  let mut mapped_any = false;

  for stream in source.property_set_streams() {
    let kind = stream.kind();
    let Some(property_sets) = (match stream.data() {
      OfficePropertySetData::Parsed(stream) => Some(&stream.property_sets),
      OfficePropertySetData::Compatibility { .. } => None,
    }) else {
      unsupported(
        report,
        options,
        ConversionCode::PropertySetNotMapped,
        SourceLocation::OfficePropertySet { kind },
      )?;
      continue;
    };

    for (property_set_index, property_set) in property_sets.iter().enumerate() {
      let (code_page, code_page_valid) = match property_set.code_page() {
        Ok(value) => (value, true),
        Err(_) => {
          unsupported(
            report,
            options,
            ConversionCode::CorePropertyNotMapped,
            SourceLocation::OfficeProperty {
              kind,
              property_set_index,
              property_identifier: PID_CODE_PAGE,
            },
          )?;
          (None, false)
        }
      };
      for property in &property_set.properties {
        let location = SourceLocation::OfficeProperty {
          kind,
          property_set_index,
          property_identifier: property.identifier,
        };
        if property.identifier == PID_CODE_PAGE {
          if code_page_valid {
            report.record(Disposition::NotApplicable);
          }
          continue;
        }

        let destination = match (kind, property_set_index, property.identifier) {
          (OfficePropertySetKind::SummaryInformation, 0, PIDSI_TITLE) => {
            StringDestination::Plain(&mut target.title)
          }
          (OfficePropertySetKind::SummaryInformation, 0, PIDSI_SUBJECT) => {
            StringDestination::Plain(&mut target.subject)
          }
          (OfficePropertySetKind::SummaryInformation, 0, PIDSI_AUTHOR) => {
            StringDestination::Creator(&mut target.creator)
          }
          (OfficePropertySetKind::SummaryInformation, 0, PIDSI_KEYWORDS) => {
            StringDestination::Keywords(&mut target.keywords)
          }
          (OfficePropertySetKind::SummaryInformation, 0, PIDSI_COMMENTS) => {
            StringDestination::Plain(&mut target.description)
          }
          (OfficePropertySetKind::SummaryInformation, 0, PIDSI_LAST_AUTHOR) => {
            StringDestination::Plain(&mut target.last_modified_by)
          }
          (OfficePropertySetKind::SummaryInformation, 0, PIDSI_REVISION_NUMBER) => {
            StringDestination::Plain(&mut target.revision)
          }
          (OfficePropertySetKind::DocumentSummaryInformation, 0, PIDDSI_CATEGORY) => {
            StringDestination::Plain(&mut target.category)
          }
          (OfficePropertySetKind::DocumentSummaryInformation, 0, PIDDSI_CONTENT_TYPE) => {
            StringDestination::Plain(&mut target.content_type)
          }
          (OfficePropertySetKind::DocumentSummaryInformation, 0, PIDDSI_CONTENT_STATUS) => {
            StringDestination::Plain(&mut target.content_status)
          }
          (OfficePropertySetKind::DocumentSummaryInformation, 0, PIDDSI_LANGUAGE) => {
            StringDestination::Language(&mut target.language)
          }
          _ => {
            unsupported(
              report,
              options,
              ConversionCode::CorePropertyNotMapped,
              location,
            )?;
            continue;
          }
        };

        let Some(value) = decoded_string(property, code_page, options, report, location)? else {
          continue;
        };
        if destination.set(value) {
          mapped_any = true;
          report.record(Disposition::Mapped);
        } else {
          unsupported(
            report,
            options,
            ConversionCode::CorePropertyNotMapped,
            location,
          )?;
        }
      }
    }
  }

  if source.vba_project().is_some() {
    unsupported(
      report,
      options,
      ConversionCode::VbaProjectNotMapped,
      SourceLocation::OfficeVbaProject,
    )?;
  }

  if mapped_any {
    target.xmlns = vec![
      XmlNamespace::known(XmlKnownNamespace::Cp),
      XmlNamespace::known(XmlKnownNamespace::Dc),
      XmlNamespace::known(XmlKnownNamespace::Dcterms),
    ];
    Ok(Some(target))
  } else {
    Ok(None)
  }
}

enum StringDestination<'a> {
  Plain(&'a mut Option<String>),
  Creator(&'a mut Option<cp::Creator>),
  Keywords(&'a mut Option<cp::Keywords>),
  Language(&'a mut Option<cp::Language>),
}

impl StringDestination<'_> {
  fn set(self, value: String) -> bool {
    match self {
      Self::Plain(destination) => set_once(destination, value),
      Self::Creator(destination) => set_once(
        destination,
        cp::Creator {
          xml_content: Some(value),
          ..Default::default()
        },
      ),
      Self::Keywords(destination) => set_once(
        destination,
        cp::Keywords {
          xml_content: Some(value),
          ..Default::default()
        },
      ),
      Self::Language(destination) => set_once(
        destination,
        cp::Language {
          xml_content: Some(value),
          ..Default::default()
        },
      ),
    }
  }
}

fn set_once<T>(destination: &mut Option<T>, value: T) -> bool {
  if destination.is_some() {
    false
  } else {
    *destination = Some(value);
    true
  }
}

fn decoded_string(
  property: &Property,
  code_page: Option<u16>,
  options: ConversionOptions,
  report: &mut ConversionReport,
  location: SourceLocation,
) -> Result<Option<String>> {
  match property.string_value(code_page) {
    Ok(Some(value)) => Ok(Some(value)),
    Ok(None) | Err(_) => {
      unsupported(
        report,
        options,
        ConversionCode::CorePropertyNotMapped,
        location,
      )?;
      Ok(None)
    }
  }
}

fn unsupported(
  report: &mut ConversionReport,
  options: ConversionOptions,
  code: ConversionCode,
  source: SourceLocation,
) -> Result<()> {
  match options.unsupported {
    LossPolicy::Reject => Err(Error::Unsupported {
      code,
      location: source,
    }),
    LossPolicy::Report => {
      report.issue(Disposition::Unsupported, code, source);
      Ok(())
    }
  }
}
