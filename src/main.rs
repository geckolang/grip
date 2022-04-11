#![deny(rust_2018_idioms)]

use futures_util::StreamExt;
use std::{collections::vec_deque, str::FromStr};
use std::{collections::vec_deque::VecDeque, io::Write};

mod build;
mod console;
mod dependency;
mod package;

// TODO: Consider replacing this to a "lex" subcommand.
const ARG_LIST_TOKENS: &str = "tokens";
const ARG_BUILD: &str = "build";
const ARG_BUILD_PRINT_OUTPUT: &str = "print";
const ARG_BUILD_NO_VERIFY: &str = "no-verify";
const ARG_BUILD_OPT: &str = "opt";
const ARG_INIT: &str = "init";
const ARG_INIT_NAME: &str = "name";
const ARG_INIT_FORCE: &str = "force";
const ARG_INSTALL: &str = "install";
const ARG_INSTALL_PATH: &str = "repository-path";
const ARG_INSTALL_BRANCH: &str = "branch";
const ARG_CHECK: &str = "check";
const ARG_CLEAN: &str = "clean";
const ARG_RUN: &str = "run";
const PATH_SOURCES: &str = "src";
const DEFAULT_OUTPUT_DIR: &str = "./build";
const PATH_DEPENDENCIES: &str = "dependencies";

async fn run() -> Result<(), String> {
  let app = clap::App::new("Grip")
  .version(clap::crate_version!())
  .author(clap::crate_authors!())
  .about("Package manager & command-line utility for the gecko programming language")
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
    .arg(clap::Arg::with_name(ARG_BUILD_NO_VERIFY).short("v").long(ARG_BUILD_NO_VERIFY).help("Skip LLVM IR verification"))
    .arg(clap::Arg::with_name(ARG_BUILD_OPT).short("O").long(ARG_BUILD_OPT).help("Specify the optimization level of the produced LLVM IR")),
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
  )
  .subcommand(clap::SubCommand::with_name(ARG_CHECK).about("Perform type-checking only"))
  .subcommand(clap::SubCommand::with_name(ARG_CLEAN).about("Clean the build directory and any produced artifacts"))
  .subcommand(clap::SubCommand::with_name(ARG_RUN).about("Build and execute the project"));

  let matches = app.get_matches();
  let llvm_context = inkwell::context::Context::create();
  let set_logger_result = log::set_logger(&console::LOGGER);

  if let Err(error) = set_logger_result {
    return Err(format!(
      "there was an error initializing the logger: {}",
      error
    ));
  }

  log::set_max_level(log::LevelFilter::Info);

  if let Some(init_arg_matches) = matches.subcommand_matches(ARG_INIT) {
    package::init_manifest(&init_arg_matches);

    Ok(())
  } else if let Some(_build_arg_matches) = matches.subcommand_matches(ARG_BUILD) {
    let package_manifest = package::fetch_manifest(&package::PATH_MANIFEST_FILE.into())?;
    let package_lock = package::get_or_init_package_lock()?;
    let llvm_module = llvm_context.create_module(package_manifest.name.as_str());
    let mut driver = build::Driver::new(&llvm_context, &llvm_module);
    let mut build_queue = std::collections::VecDeque::new();
    let mut is_initial_package = true;

    build_queue.push_front(package_manifest.clone());

    while let Some(package) = build_queue.pop_front() {
      if package.ty == package::PackageType::Executable && !is_initial_package {
        return Err("dependency is an executable, but was expected to be a library".to_string());
      }

      let sources_dir = if is_initial_package {
        let result = std::path::PathBuf::from(PATH_SOURCES);

        is_initial_package = false;

        result
      } else {
        std::path::PathBuf::from(package::PATH_DEPENDENCIES)
          .join(package.name.clone())
          .join(PATH_SOURCES)
      };

      let source_directories = package::read_sources_dir(&sources_dir)?;

      // TODO: Shouldn't these source files be saved under a package (HashMap)?
      for source_file in source_directories {
        driver
          .source_files
          .push((package.name.clone(), source_file));
      }

      // TODO: Handle cyclic dependencies.
      // Add dependencies to build queue.
      for dependency in &package.dependencies {
        let dependency_manifest = package::fetch_dependency_manifest(dependency)?;

        build_queue.push_front(dependency_manifest);
      }
    }

    // TODO: Use a map to store the sources, then read it here
    // and provide it to the project builder to link diagnostics
    // to specific files (via `(source_file_name, diagnostic)`).

    let diagnostics = driver.build();

    for diagnostic in diagnostics {
      // TODO: Maybe fix this by clearing then re-writing the progress bar.
      // FIXME: This will interfere with the progress bar (leave it behind).
      crate::console::print_diagnostic(
        vec![(
          // TODO:
          &"source_file_path_here_pending".to_string(),
          // FIXME:
          &"source_file_path_contents_here_pending".to_string(),
        )],
        &diagnostic,
      );
    }

    llvm_module.set_triple(&inkwell::targets::TargetMachine::get_default_triple());

    let llvm_ir = llvm_module.print_to_string().to_string();
    let default_output_path = std::path::PathBuf::from(DEFAULT_OUTPUT_DIR);
    let mut output_path = default_output_path.clone();

    output_path.push(package_manifest.name);
    output_path.set_extension("ll");

    if !default_output_path.exists() && std::fs::create_dir(crate::DEFAULT_OUTPUT_DIR).is_err() {
      log::error!("failed to create output directory");
    } else if let Err(error) = std::fs::write(output_path, llvm_ir) {
      log::error!("failed to write output file: {}", error);
    }

    Ok(())
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
      return Err(format!(
        "failed to fetching the package manifest file: {}",
        error
      ));
    }

    let package_manifest_file_response = package_manifest_file_response_result.unwrap();

    if package_manifest_file_response.status() == reqwest::StatusCode::NOT_FOUND {
      return Err(String::from(
        "the package manifest file was not found on the requested repository",
      ));
    } else if !package_manifest_file_response.status().is_success() {
      return Err(format!(
        "failed to fetching the package manifest file: HTTP error {}",
        package_manifest_file_response.status()
      ));
    }

    let package_manifest_file_text = package_manifest_file_response.text().await;

    if let Err(error) = package_manifest_file_text {
      return Err(format!(
        "failed to fetching the package manifest file: {}",
        error
      ));
    }

    let package_manifest_result =
      toml::from_str::<package::Manifest>(package_manifest_file_text.unwrap().as_str());

    if let Err(error) = package_manifest_result {
      return Err(format!(
        "failed to parse the package manifest file: {}",
        error
      ));
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
        return Err(format!("failed to download the package: {}", error));
      }

      response_result.unwrap()
    };

    if !package_zip_file_response.status().is_success() {
      return Err(format!(
        "failed to download the package: HTTP error {}",
        package_zip_file_response.status()
      ));
    }

    let file_size = {
      let content_length = package_zip_file_response.content_length();

      // FIXME: Getting fragile `failed to download the package: no content length` errors.
      if content_length.is_none() {
        return Err("failed to download the package: no content length".to_string());
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
        return Err(format!(
          "failed to create the dependencies directory: {}",
          error
        ));
      }
    }

    file_path.push(format!("{}.zip", package_manifest.name));

    let mut file = {
      let file_result = std::fs::File::create(file_path);

      if let Err(error) = file_result {
        progress_bar.finish_and_clear();

        return Err(format!(
          "failed to create output file for package download: {}",
          error
        ));
      }

      file_result.unwrap()
    };

    let mut downloaded_bytes: u64 = 0;
    let mut bytes_stream = package_zip_file_response.bytes_stream();

    while let Some(chunk_result) = bytes_stream.next().await {
      if let Err(error) = chunk_result {
        progress_bar.finish_and_clear();

        return Err(format!("failed to download the package: {}", error));
      }

      let chunk = chunk_result.unwrap();

      if let Err(error) = file.write(&chunk) {
        progress_bar.finish_and_clear();

        return Err(format!("failed to write to output file: {}", error));
      }

      let new_progress_position = std::cmp::min(downloaded_bytes + (chunk.len() as u64), file_size);

      downloaded_bytes = new_progress_position;
      progress_bar.set_position(new_progress_position);
    }

    progress_bar.finish_and_clear();
    log::info!("downloaded package `{}`", package_manifest.name);

    Ok(())

    // TODO: Continue implementation: unzip and process the downloaded package.
  } else {
    // TODO:
    // clap.Error::with_description("no file specified", clap::ErrorKind::MissingArgument);
    Err("try running `grip --help`".to_string())
    // app.print_long_help();
  }
}

#[tokio::main]
async fn main() {
  match run().await {
    Ok(_) => (),
    Err(error_message) => {
      log::error!("{}", error_message);
      std::process::exit(1);
    }
  }
}

// TODO: Consider expanding this function (or re-structuring it).
fn print_or_write_output(output: String, output_file_path: &std::path::PathBuf, print: bool) {
  if print {
    println!("{}", output);
  } else if let Err(error) = std::fs::write(output_file_path, output) {
    log::error!("failed to write output file: {}", error);
  }
}
