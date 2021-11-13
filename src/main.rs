mod console;
mod package;

const ARG_FILE: &str = "file";
const ARG_LIST_TOKENS: &str = "list-tokens";
const ARG_PRINT_LLVM_IR: &str = "print-llvm-ir";
const ARG_BUILD: &str = "build";
const ARG_BUILD_SOURCES_DIR: &str = "sources-dir";
const ARG_BUILD_OUTPUT_DIR: &str = "output-dir";
const ARG_INIT: &str = "init";
const ARG_INIT_NAME: &str = "name";
const ARG_INIT_FORCE: &str = "force";
const DEFAULT_SOURCES_DIR: &str = "src";
const DEFAULT_OUTPUT_DIR: &str = "build";

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
      clap::SubCommand::with_name(ARG_BUILD)
        .about("Build the project in the current directory")
        .arg(
          clap::Arg::with_name(ARG_BUILD_SOURCES_DIR)
            .short("s")
            .long(ARG_BUILD_SOURCES_DIR)
            .default_value(DEFAULT_SOURCES_DIR),
        )
        .arg(
          clap::Arg::with_name(ARG_BUILD_OUTPUT_DIR)
            .short("o")
            .long(ARG_BUILD_OUTPUT_DIR)
            .default_value(DEFAULT_OUTPUT_DIR),
        ),
    )
    .subcommand(
      clap::SubCommand::with_name(ARG_INIT)
        .about("Initialize a default package manifest file in the current directory")
        .arg(clap::Arg::with_name(ARG_INIT_NAME).default_value("project"))
        .arg(
          clap::Arg::with_name(ARG_INIT_FORCE)
            .help("Reinitialize an existing package manifest file if applicable")
            .short("f")
            .long(ARG_INIT_FORCE),
        ),
    );

  // FIXME: Need to implement log crate (it is a facade).

  let matches = app.get_matches();
  let llvm_context = inkwell::context::Context::create();

  let set_logger_result = log::set_logger(&console::LOGGER);

  if let Err(error) = set_logger_result {
    // TODO: Special case.
    println!("there was an error initializing the logger: {}", error);

    return;
  }

  log::set_max_level(log::LevelFilter::Info);

  if matches.subcommand_matches(ARG_INIT).is_some() {
    package::init_package_manifest(&matches);

    return;
  } else if matches.subcommand_matches(ARG_BUILD).is_some() {
    let build_result = package::build_package(&llvm_context, &matches);

    if build_result.is_some() {
      let build_result_tuple = build_result.unwrap();

      let mut final_output_path = std::path::PathBuf::from(
        matches
          .subcommand_matches(ARG_BUILD)
          .unwrap()
          .value_of(ARG_BUILD_OUTPUT_DIR)
          .unwrap(),
      );

      final_output_path.push(build_result_tuple.1);
      write_or_print_output(build_result_tuple.0, &final_output_path, &matches);
    }

    return;
  } else if !matches.is_present(ARG_FILE) {
    // TODO:
    // clap.Error::with_description("no file specified", clap::ErrorKind::MissingArgument);
    log::error!("try running `grip --help`");
    // app.print_long_help();

    return;
  }

  let source_file_path = std::path::PathBuf::from(matches.value_of(ARG_FILE).unwrap());
  let llvm_context = inkwell::context::Context::create();

  let llvm_module =
      // TODO: Need to verify that `source_file_path` is a file path, otherwise `.file_stem()` might return `None`.
      // TODO: Prefer usage of `.file_prefix()` once it is stable.
      llvm_context.create_module(source_file_path.file_stem().unwrap().to_str().unwrap());

  let source_file_contents_result = package::fetch_source_file_contents(&source_file_path.clone());

  if let Err(error) = source_file_contents_result {
    log::error!("failed to read source file contents: {}", error);

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
    console::print_diagnostic(
      vec![(
        &source_file_path.clone().to_str().unwrap().to_string(),
        &source_file_contents,
      )],
      &build_result.err().unwrap(),
    );
    // println!("{}", to_codespan_reporting_diagnostic(build_result.err()));
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
    if let Err(error) = std::fs::write(output_file_path, llvm_ir) {
      log::error!("failed to write output file: {}", error);
    }
  }
}
