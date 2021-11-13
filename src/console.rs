pub struct Logger;

pub static LOGGER: Logger = Logger;

impl log::Log for Logger {
  fn enabled(&self, metadata: &log::Metadata) -> bool {
    metadata.level() <= log::Level::Info
  }

  fn log(&self, record: &log::Record) {
    if self.enabled(record.metadata()) {
      // TODO: Use lighter colors.

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

fn to_codespan_reporting_diagnostic<T>(
  diagnostic: &gecko::diagnostic::Diagnostic,
) -> codespan_reporting::diagnostic::Diagnostic<T> {
  codespan_reporting::diagnostic::Diagnostic::new(match diagnostic.severity {
    gecko::diagnostic::DiagnosticSeverity::Error => codespan_reporting::diagnostic::Severity::Error,
    gecko::diagnostic::DiagnosticSeverity::Warning => {
      codespan_reporting::diagnostic::Severity::Warning
    }
    gecko::diagnostic::DiagnosticSeverity::Internal => {
      codespan_reporting::diagnostic::Severity::Bug
    }
  })
  .with_message(diagnostic.message.clone())
}

pub fn print_diagnostic(
  files: Vec<(&String, &String)>,
  diagnostic: &gecko::diagnostic::Diagnostic,
) {
  let writer = codespan_reporting::term::termcolor::StandardStream::stderr(
    codespan_reporting::term::termcolor::ColorChoice::Always,
  );

  let config = codespan_reporting::term::Config::default();
  let mut codespan_files = codespan_reporting::files::SimpleFiles::new();
  let codespan_diagnostic = to_codespan_reporting_diagnostic(diagnostic);

  for file in files {
    codespan_files.add(file.0, file.1);
  }

  // TODO: Handle possible error.
  codespan_reporting::term::emit(
    &mut writer.lock(),
    &config,
    &codespan_files,
    &codespan_diagnostic,
  );
}
