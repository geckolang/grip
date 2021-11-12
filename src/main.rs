extern crate clap;
extern crate gecko;

mod package;

const ARG_FILE: &str = "file";
const ARG_LIST_TOKENS: &str = "list-tokens";
const ARG_PRINT_LLVM_IR: &str = "print-llvm-ir";
const ARG_BUILD: &str = "build";
const ARG_INIT: &str = "init";

fn main() {
  let app = clap::App::new("Grip")
    .version(clap::crate_version!())
    .author(clap::crate_authors!())
    .about("Package manager & command-line utility for the gecko programming language")
    .arg(
      clap::Arg::with_name(ARG_FILE)
        .help("The file to process")
        .index(1),
    )
    .arg(
      clap::Arg::with_name(ARG_LIST_TOKENS)
        .short("t")
        .long(ARG_LIST_TOKENS)
        .help("Display a list of the lexed tokens"),
    )
    .arg(
      clap::Arg::with_name(ARG_PRINT_LLVM_IR)
        .short("i")
        .long(ARG_PRINT_LLVM_IR)
        .help("Print the resulting LLVM IR instead of producing an output file"),
    )
    .subcommand(
      clap::SubCommand::with_name(ARG_BUILD).about("Build the project in the current directory"),
    )
    .subcommand(
      clap::SubCommand::with_name(ARG_INIT)
        .about("Initialize a default package manifest file in the current directory"),
    );

  let matches = app.get_matches();
  let llvm_context = inkwell::context::Context::create();

  if matches.subcommand_matches(ARG_INIT).is_some() {
    package::init_package_manifest();

    return;
  } else if matches.subcommand_matches(ARG_BUILD).is_some() {
    let build_result = package::build_package(&llvm_context, &matches);

    if build_result.is_some() {
      let build_result_tuple = build_result.unwrap();
      let mut final_output_path = std::path::PathBuf::from(package::PATH_OUTPUT_DIRECTORY);

      final_output_path.push(build_result_tuple.1);

      write_or_print_output(build_result_tuple.0, &final_output_path, &matches);
    }

    return;
  } else if !matches.is_present(ARG_FILE) {
    // TODO:
    // clap.Error::with_description("no file specified", clap::ErrorKind::MissingArgument);
    println!("try running --help");
    // app.print_long_help();

    return;
  } else {
    println!("building single file");

    let source_file_path = std::path::PathBuf::from(matches.value_of(ARG_FILE).unwrap());
    let llvm_context = inkwell::context::Context::create();

    let llvm_module =
      // TODO: Need to verify that `source_file_path` is a file path, otherwise `.file_stem()` might return `None`.
      // TODO: Prefer usage of `.file_prefix()` once it is stable.
      llvm_context.create_module(source_file_path.file_stem().unwrap().to_str().unwrap());

    let source_file_contents_result =
      package::fetch_source_file_contents(&source_file_path.clone());

    if let Err(error_message) = source_file_contents_result {
      println!("{}", error_message);

      return;
    }

    let source_file_contents = source_file_contents_result.unwrap();

    let build_result =
      package::build_single_file(&llvm_context, &llvm_module, &source_file_contents, &matches);

    if build_result.is_ok() {
      let mut output_file_path = std::path::PathBuf::from(source_file_path.parent().unwrap());

      output_file_path.push(source_file_path.file_stem().unwrap());
      output_file_path.set_extension(package::PATH_OUTPUT_FILE_EXTENSION);
      write_or_print_output(llvm_module, &output_file_path, &matches);
    } else {
      print_diagnostic(
        vec![(
          &source_file_path.clone().to_str().unwrap().to_string(),
          &source_file_contents,
        )],
        &build_result.err().unwrap(),
      );
      // println!("{}", to_codespan_reporting_diagnostic(build_result.err()));
    }
  }
}

fn write_or_print_output(
  llvm_module: inkwell::module::Module,
  output_file_path: &std::path::PathBuf,
  matches: &clap::ArgMatches,
) {
  let llvm_ir = llvm_module.print_to_string().to_string();

  if matches.is_present(crate::ARG_PRINT_LLVM_IR) {
    println!("{}", llvm_ir);
  } else {
    if let Err(error_message) = std::fs::write(output_file_path, llvm_ir) {
      println!("{}", error_message);
    }
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

fn print_diagnostic(files: Vec<(&String, &String)>, diagnostic: &gecko::diagnostic::Diagnostic) {
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
