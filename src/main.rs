#![deny(rust_2018_idioms)]

use futures_util::StreamExt;
use std::io::Write;

mod build;
mod console;
mod package;

const ARG_FILE: &str = "file";
const ARG_LIST_TOKENS: &str = "tokens";
const ARG_BUILD: &str = "build";
const ARG_BUILD_PRINT_OUTPUT: &str = "print";
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
async fn main() -> Result<(), i32> {
  let app = clap::App::new("Grip")
    .version(clap::crate_version!())
    .author(clap::crate_authors!())
    .about("Package manager & command-line utility for the gecko programming language")
    // TODO: Make this a positional under the `build` subcommand.
    .arg(
      clap::Arg::with_name(ARG_FILE)
        .help("The file to process")
        .index(1),
    )
    .subcommand(
      clap::SubCommand::with_name(ARG_BUILD)
        .about("Build the project in the current directory")
        .arg(
          clap::Arg::with_name(ARG_LIST_TOKENS)
            .short("t")
            .long(ARG_LIST_TOKENS)
            .help("Display a list of the lexed tokens"),
        )
        .arg(
          clap::Arg::with_name(ARG_BUILD_PRINT_OUTPUT)
            .short("p")
            .long(ARG_BUILD_PRINT_OUTPUT)
            .help("Print the resulting LLVM IR instead of producing an output file"),
        )
    )
    .subcommand(
      clap::SubCommand::with_name(ARG_INIT)
        .about("Initialize a default package manifest file in the current directory")
        .arg(clap::Arg::with_name(ARG_INIT_NAME).default_value("project").index(1))
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
    eprintln!("there was an error initializing the logger: {}", error);

    return Err(1);
  }

  log::set_max_level(log::LevelFilter::Info);

  if let Some(init_arg_matches) = matches.subcommand_matches(ARG_INIT) {
    package::init_manifest(&init_arg_matches);
  } else if let Some(build_arg_matches) = matches.subcommand_matches(ARG_BUILD) {
    let build_result = build::build_package(&llvm_context, &build_arg_matches);

    if let Ok(build_result_tuple) = build_result {
      let mut final_output_path = std::path::PathBuf::from(DEFAULT_OUTPUT_DIR);

      final_output_path.push(build_result_tuple.1);

      print_or_write_output(
        build_result_tuple.0,
        &final_output_path,
        build_arg_matches.is_present(ARG_BUILD_PRINT_OUTPUT),
      );
    } else {
      return Err(1);
    }
  } else if let Some(_check_arg_matches) = matches.subcommand_matches(ARG_CHECK) {
    // TODO: Implement.
    todo!();
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

      return Err(1);
    }

    let package_manifest_file_response = package_manifest_file_response_result.unwrap();

    if package_manifest_file_response.status() == reqwest::StatusCode::NOT_FOUND {
      log::error!("the package manifest file was not found on the requested repository");

      return Err(1);
    } else if !package_manifest_file_response.status().is_success() {
      log::error!(
        "failed to fetching the package manifest file: HTTP error {}",
        package_manifest_file_response.status()
      );

      return Err(1);
    }

    let package_manifest_file_text = package_manifest_file_response.text().await;

    if let Err(error) = package_manifest_file_text {
      log::error!("failed to fetching the package manifest file: {}", error);

      return Err(1);
    }

    let package_manifest_result =
      toml::from_str::<package::PackageManifest>(package_manifest_file_text.unwrap().as_str());

    if let Err(error) = package_manifest_result {
      log::error!("failed to parse the package manifest file: {}", error);

      return Err(1);
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

        return Err(1);
      }

      response_result.unwrap()
    };

    if !package_zip_file_response.status().is_success() {
      log::error!(
        "failed to download the package: HTTP error {}",
        package_zip_file_response.status()
      );

      return Err(1);
    }

    let file_size = {
      let content_length = package_zip_file_response.content_length();

      // FIXME: Getting fragile `failed to download the package: no content length` errors.
      if content_length.is_none() {
        log::error!("failed to download the package: no content length");

        return Err(1);
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

        return Err(1);
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

        return Err(1);
      }

      file_result.unwrap()
    };

    let mut downloaded_bytes: u64 = 0;
    let mut bytes_stream = package_zip_file_response.bytes_stream();

    while let Some(chunk_result) = bytes_stream.next().await {
      if let Err(error) = chunk_result {
        progress_bar.finish_and_clear();
        log::error!("failed to download the package: {}", error);

        return Err(1);
      }

      let chunk = chunk_result.unwrap();

      if let Err(error) = file.write(&chunk) {
        progress_bar.finish_and_clear();
        log::error!("failed to write to output file: {}", error);

        return Err(1);
      }

      let new_progress_position = std::cmp::min(downloaded_bytes + (chunk.len() as u64), file_size);

      downloaded_bytes = new_progress_position;
      progress_bar.set_position(new_progress_position);
    }

    progress_bar.finish_and_clear();
    log::info!("downloaded package `{}`", package_manifest.name);

    // TODO: Continue implementation: unzip and process the downloaded package.
  } else if matches.is_present(ARG_FILE) {
    // TODO: Make this positional under `build` subcommand instead.

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

      return Err(1);
    }

    let source_file_contents = source_file_contents_result.unwrap();

    // TODO: File names need to conform to identifier rules.

    let build_diagnostics = build::build_single_file(
      &llvm_context,
      &llvm_module,
      source_file_path
        .file_stem()
        .unwrap()
        .to_string_lossy()
        .to_string(),
      &source_file_contents,
      &matches,
    );

    // TODO: What if its just non-erroneous diagnostics?
    if !build_diagnostics.is_empty() {
      for diagnostic in build_diagnostics {
        console::print_diagnostic(
          source_file_contents.as_str(),
          vec![(
            &source_file_path.clone().to_str().unwrap().to_string(),
            &source_file_contents,
          )],
          &diagnostic,
        );
      }

      return Err(1);
    }

    let mut output_file_path = std::path::PathBuf::from(source_file_path.parent().unwrap());

    output_file_path.push(source_file_path.file_stem().unwrap());
    output_file_path.set_extension(build::PATH_OUTPUT_FILE_EXTENSION);

    // TODO: Use `ARG_BUILD_PRINT_OUTPUT` after being a positional under `build` subcommand.
    print_or_write_output(
      llvm_module.print_to_string().to_string(),
      &output_file_path,
      false,
    );
  } else {
    // TODO:
    // clap.Error::with_description("no file specified", clap::ErrorKind::MissingArgument);
    log::error!("try running `grip --help`");
    // app.print_long_help();

    return Err(1);
  }

  Ok(())
}

// TODO: Consider expanding this function (or re-structuring it).
fn print_or_write_output(output: String, output_file_path: &std::path::PathBuf, print: bool) {
  if print {
    // NOTE: The newline is to separate from the build completion message.
    print!("\n{}", output);
  } else if let Err(error) = std::fs::write(output_file_path, output) {
    log::error!("failed to write output file: {}", error);
  }
}
