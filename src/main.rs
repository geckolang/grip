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

  if matches.subcommand_matches(ARG_INIT).is_some() {
    package::init_package_manifest();

    return;
  } else if matches.subcommand_matches(ARG_BUILD).is_some() {
    package::build_package(&matches);

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

    if package::build_single_file(
      &llvm_context,
      llvm_module.clone(),
      source_file_path.clone(),
      &matches,
    ) {
      let mut output_file_path = std::path::PathBuf::from(source_file_path.parent().unwrap());

      output_file_path.push(source_file_path.file_stem().unwrap());
      output_file_path.set_extension(package::PATH_OUTPUT_FILE_EXTENSION);
      write_or_print_output(llvm_module, output_file_path, &matches);
    }
  }
}

fn write_or_print_output(
  llvm_module: inkwell::module::Module,
  output_file_path: std::path::PathBuf,
  matches: &clap::ArgMatches,
) {
  let llvm_ir = llvm_module.print_to_string().to_string();

  if matches.is_present(crate::ARG_PRINT_LLVM_IR) {
    println!("{}", llvm_ir);
  } else {
    if std::fs::write(output_file_path, llvm_ir).is_err() {
      println!("failed to write output file");
    }
  }
}
