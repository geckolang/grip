pub struct Logger;

pub static LOGGER: Logger = Logger;

impl log::Log for Logger {
  fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
    metadata.level() <= log::Level::Info
  }

  fn log(&self, record: &log::Record<'_>) {
    if self.enabled(record.metadata()) {
      // TODO: Use lighter colors.
      // TODO: There is an extra newline at the end of the message. Is this causing it?

      println!(
        // TODO: Width not working because of the color codes.
        "{:>7}: {}",
        match record.level() {
          log::Level::Error => ansi_term::Colour::Red.paint("error"),
          log::Level::Warn => ansi_term::Colour::Yellow.paint("warning"),
          log::Level::Info => ansi_term::Colour::Cyan.paint("info"),
          log::Level::Debug => ansi_term::Colour::Purple.paint("debug"),
          log::Level::Trace => ansi_term::Colour::White.paint("trace"),
        },
        record.args()
      );
    }
  }

  fn flush(&self) {
    //
  }
}

pub fn print_diagnostic(
  files: Vec<(&String, &String)>,
  diagnostic: &gecko::diagnostic::Diagnostic,
) {
  let writer = codespan_reporting::term::termcolor::StandardStream::stderr(
    codespan_reporting::term::termcolor::ColorChoice::Auto,
  );

  let config = codespan_reporting::term::Config::default();
  let mut codespan_files = codespan_reporting::files::SimpleFiles::new();

  let mut codespan_diagnostic =
    codespan_reporting::diagnostic::Diagnostic::new(match diagnostic.severity {
      gecko::diagnostic::Severity::Error => codespan_reporting::diagnostic::Severity::Error,
      gecko::diagnostic::Severity::Warning => codespan_reporting::diagnostic::Severity::Warning,
      gecko::diagnostic::Severity::Internal => codespan_reporting::diagnostic::Severity::Bug,
    })
    .with_message(diagnostic.message.clone());

  // Display the source (if applicable).
  if let Some(span) = &diagnostic.span {
    // TODO: Is there a need to re-assign here?
    codespan_diagnostic =
      codespan_diagnostic.with_labels(vec![codespan_reporting::diagnostic::Label::primary(
        // FIXME: Temporary value. Need actual file id (is this the index?).
        0,
        span.clone(),
      )]);
  }

  if diagnostic.severity == gecko::diagnostic::Severity::Internal {
    codespan_diagnostic
      .notes
      .push("please report this to the compiler team".into());
  }

  for file in files {
    codespan_files.add(file.0, file.1);
  }

  let emit_result = codespan_reporting::term::emit(
    &mut writer.lock(),
    &config,
    &codespan_files,
    &codespan_diagnostic,
  );

  if let Err(error) = emit_result {
    eprintln!("failed to emit diagnostic to the console: {}", error);
  }
}
