use futures_util::StreamExt;
use std::io::Write;

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
const ARG_INSTALL: &str = "install";
const ARG_INSTALL_URL: &str = "url";
const DEFAULT_SOURCES_DIR: &str = "src";
const DEFAULT_OUTPUT_DIR: &str = "build";
const PATH_DEPENDENCIES: &str = "dependencies";

#[tokio::main]
async fn main() {
  let app = clap::App::new("Grip")
    .version(clap::crate_version!())
    .author(clap::crate_authors!())
    .about("Package manager & command-line utility for the gecko programming language")
    .arg(
      // TODO: Take in a list of files instead.
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
    )
    .subcommand(
      clap::SubCommand::with_name(ARG_INSTALL)
        .about("Install a package")
        .arg(clap::Arg::with_name(ARG_INSTALL_URL).index(1)),
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
  } else if let Some(install_arg_matches) = matches.subcommand_matches(ARG_INSTALL) {
    // TODO: Might need a client for subsequent dependencies of the installed package.
    let reqwest_client = reqwest::Client::new();

    let response = {
      let response_result = reqwest_client
        .get(install_arg_matches.value_of(ARG_INSTALL_URL).unwrap())
        .send()
        .await;

      if let Err(error) = response_result {
        log::error!("failed to download the package: {}", error);

        return;
      }

      response_result.unwrap()
    };

    if !response.status().is_success() {
      log::error!(
        "failed to download the package: HTTP error {}",
        response.status()
      );

      return;
    }

    let file_size = {
      let content_length = response.content_length();

      if content_length.is_none() {
        log::error!("failed to download the package: no content length");

        return;
      }

      content_length.unwrap()
    };

    let progress_bar = indicatif::ProgressBar::new(file_size);

    progress_bar.set_style(indicatif::ProgressStyle::default_bar().template(
      "downloading package: {msg} [{bar:30}] {bytes}/{total_bytes} {bytes_per_sec}, {eta}",
    ));

    progress_bar.set_message(
      // TODO: Use package name instead of install url.
      install_arg_matches
        .value_of(ARG_INSTALL_URL)
        .unwrap()
        .to_string(),
    );

    let mut file_path = std::path::PathBuf::from(PATH_DEPENDENCIES);

    file_path.push(".downloading");

    if !file_path.exists() {
      std::fs::create_dir_all(file_path.clone());
    }

    // FIXME: Temporary.
    file_path.push("my-dep.zip");

    let mut file = {
      let file_result = std::fs::File::create(file_path);

      if let Err(error) = file_result {
        log::error!(
          "failed to create output file for package download: {}",
          error
        );

        return;
      }

      file_result.unwrap()
    };

    let mut downloaded_bytes: u64 = 0;
    let mut bytes_stream = response.bytes_stream();

    while let Some(chunk_result) = bytes_stream.next().await {
      if let Err(error) = chunk_result {
        log::error!("failed to download the package: {}", error);

        return;
      }

      let chunk = chunk_result.unwrap();

      if let Err(error) = file.write(&chunk) {
        log::error!("failed to write to output file: {}", error);

        return;
      }

      let new_progress_position = std::cmp::min(downloaded_bytes + (chunk.len() as u64), file_size);

      downloaded_bytes = new_progress_position;
      progress_bar.set_position(new_progress_position);
    }

    progress_bar.finish_and_clear();

    // TODO: Use package name.
    log::info!("downloaded package `{}`", "foo");
  } else if matches.is_present(ARG_FILE) {
    let source_file_path = std::path::PathBuf::from(matches.value_of(ARG_FILE).unwrap());
    let llvm_context = inkwell::context::Context::create();

    let llvm_module =
      // TODO: Need to verify that `source_file_path` is a file path, otherwise `.file_stem()` might return `None`.
      // TODO: Prefer usage of `.file_prefix()` once it is stable.
      llvm_context.create_module(source_file_path.file_stem().unwrap().to_str().unwrap());

    let source_file_contents_result =
      package::fetch_source_file_contents(&source_file_path.clone());

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
  } else {
    // TODO:
    // clap.Error::with_description("no file specified", clap::ErrorKind::MissingArgument);
    log::error!("try running `grip --help`");
    // app.print_long_help();
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
