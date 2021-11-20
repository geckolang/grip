#![deny(rust_2018_idioms)]

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
const ARG_INSTALL_PATH: &str = "repository-path";
const ARG_INSTALL_BRANCH: &str = "branch";
const ARG_CHECK: &str = "check";
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
        .about("Install a package from a GitHub repository")
        .arg(
          clap::Arg::with_name(ARG_INSTALL_PATH)
            .index(1)
            .help("The GitHub repository path where the package lives, in the following format: `user/repository` or `organization/repository`"),
        )
        .arg(
          clap::Arg::with_name(ARG_INSTALL_BRANCH)
            .help("The GitHub repository's branch to use")
            .short("b")
            .long(ARG_INSTALL_BRANCH)
            .default_value("master"),
        ),
    ).subcommand(clap::SubCommand::with_name(ARG_CHECK).about("Perform type-checking only upon the project"));

  let matches = app.get_matches();
  let llvm_context = inkwell::context::Context::create();
  let set_logger_result = log::set_logger(&console::LOGGER);

  if let Err(error) = set_logger_result {
    // TODO: Special case.
    println!("there was an error initializing the logger: {}", error);

    return;
  }

  log::set_max_level(log::LevelFilter::Info);

  if let Some(_init_arg_matches) = matches.subcommand_matches(ARG_INIT) {
    // TODO: Pass in & process `init_arg_matches` instead of `matches`.
    package::init_package_manifest(&matches);

    return;
  } else if let Some(_build_arg_matches) = matches.subcommand_matches(ARG_BUILD) {
    // TODO: Pass in & process `build_arg_matches` instead of `matches`.
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
    let reqwest_client = reqwest::Client::new();
    let github_repository_path = install_arg_matches.value_of(ARG_INSTALL_PATH).unwrap();
    let github_branch = install_arg_matches.value_of(ARG_INSTALL_BRANCH).unwrap();

    // TODO: GitHub might be caching results from this url.
    let package_manifest_file_response_result = reqwest_client
      .get(format!(
        "https://raw.githubusercontent.com/{}/{}/{}",
        github_repository_path,
        github_branch,
        package::PATH_MANIFEST_FILE
      ))
      .send()
      .await;

    if let Err(error) = package_manifest_file_response_result {
      log::error!("failed to fetching the package manifest file: {}", error);

      return;
    }

    let package_manifest_file_response = package_manifest_file_response_result.unwrap();

    if package_manifest_file_response.status() == reqwest::StatusCode::NOT_FOUND {
      log::error!("the package manifest file was not found on the requested repository");

      return;
    } else if !package_manifest_file_response.status().is_success() {
      log::error!(
        "failed to fetching the package manifest file: HTTP error {}",
        package_manifest_file_response.status()
      );

      return;
    }

    let package_manifest_file_text = package_manifest_file_response.text().await;

    if let Err(error) = package_manifest_file_text {
      log::error!("failed to fetching the package manifest file: {}", error);

      return;
    }

    let package_manifest_result =
      toml::from_str::<package::PackageManifest>(package_manifest_file_text.unwrap().as_str());

    if let Err(error) = package_manifest_result {
      log::error!("failed to parse the package manifest file: {}", error);

      return;
    }

    let package_manifest = package_manifest_result.unwrap();

    let package_zip_file_response = {
      let response_result = reqwest_client
        .get(format!(
          "https://codeload.github.com/{}/zip/refs/heads/{}",
          github_repository_path, github_branch
        ))
        .send()
        .await;

      if let Err(error) = response_result {
        log::error!("failed to download the package: {}", error);

        return;
      }

      response_result.unwrap()
    };

    if !package_zip_file_response.status().is_success() {
      log::error!(
        "failed to download the package: HTTP error {}",
        package_zip_file_response.status()
      );

      return;
    }

    let file_size = {
      let content_length = package_zip_file_response.content_length();

      // FIXME: Getting fragile `failed to download the package: no content length` errors.
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

    progress_bar.set_message(package_manifest.name.clone());

    let mut file_path = std::path::PathBuf::from(PATH_DEPENDENCIES);

    file_path.push(".downloading");

    if !file_path.exists() {
      if let Err(error) = std::fs::create_dir_all(file_path.clone()) {
        log::error!("failed to create the dependencies directory: {}", error);

        return;
      }
    }

    file_path.push(format!("{}.zip", package_manifest.name));

    let mut file = {
      let file_result = std::fs::File::create(file_path);

      if let Err(error) = file_result {
        progress_bar.finish_and_clear();

        log::error!(
          "failed to create output file for package download: {}",
          error
        );

        return;
      }

      file_result.unwrap()
    };

    let mut downloaded_bytes: u64 = 0;
    let mut bytes_stream = package_zip_file_response.bytes_stream();

    while let Some(chunk_result) = bytes_stream.next().await {
      if let Err(error) = chunk_result {
        progress_bar.finish_and_clear();
        log::error!("failed to download the package: {}", error);

        return;
      }

      let chunk = chunk_result.unwrap();

      if let Err(error) = file.write(&chunk) {
        progress_bar.finish_and_clear();
        log::error!("failed to write to output file: {}", error);

        return;
      }

      let new_progress_position = std::cmp::min(downloaded_bytes + (chunk.len() as u64), file_size);

      downloaded_bytes = new_progress_position;
      progress_bar.set_position(new_progress_position);
    }

    progress_bar.finish_and_clear();
    log::info!("downloaded package `{}`", package_manifest.name);

    // TODO: Unzip and process it.
  } else if let Some(_check_arg_matches) = matches.subcommand_matches(ARG_CHECK) {
    // TODO: Implement.
    todo!();
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

    if let Err(diagnostics) = build_result {
      for diagnostic in diagnostics {
        console::print_diagnostic(
          vec![(
            &source_file_path.clone().to_str().unwrap().to_string(),
            &source_file_contents,
          )],
          &diagnostic,
        );
      }

      return;
    }

    let mut output_file_path = std::path::PathBuf::from(source_file_path.parent().unwrap());

    output_file_path.push(source_file_path.file_stem().unwrap());
    output_file_path.set_extension(package::PATH_OUTPUT_FILE_EXTENSION);
    write_or_print_output(llvm_module, &output_file_path, &matches);
  } else {
    // TODO:
    // clap.Error::with_description("no file specified", clap::ErrorKind::MissingArgument);
    log::error!("try running `grip --help`");
    // app.print_long_help();
  }
}

fn write_or_print_output(
  llvm_module: inkwell::module::Module<'_>,
  output_file_path: &std::path::PathBuf,
  matches: &clap::ArgMatches<'_>,
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
