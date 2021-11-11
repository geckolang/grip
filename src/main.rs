extern crate clap;
extern crate gecko;

use clap::{App, Arg, SubCommand};
use gecko::pass::*;
use serde::{Deserialize, Serialize};

const ARG_FILE: &str = "file";
const ARG_LIST_TOKENS: &str = "list-tokens";
const ARG_PRINT_LLVM_IR: &str = "print-llvm-ir";
const ARG_BUILD: &str = "build";
const ARG_INIT: &str = "init";
const PATH_MANIFEST_FILE: &str = "grip.toml";
const PATH_SOURCE_FILE_EXTENSION: &str = "ko";
const PATH_OUTPUT_FILE_EXTENSION: &str = "ll";

#[derive(Serialize, Deserialize)]
struct PackageManifest {
  name: String,
  version: String,
}

fn find_top_level_node_name(top_level_node: &gecko::node::AnyTopLevelNode) -> String {
  match top_level_node {
    gecko::node::AnyTopLevelNode::Function(function) => function.prototype.name.clone(),
    gecko::node::AnyTopLevelNode::External(external) => external.prototype.name.clone(),
  }
}

fn write_or_print_output(
  llvm_ir: String,
  output_file_path: std::path::PathBuf,
  matches: &clap::ArgMatches,
) {
  if matches.is_present(ARG_PRINT_LLVM_IR) {
    println!("{}", llvm_ir);
  } else {
    if std::fs::write(output_file_path, llvm_ir).is_err() {
      println!("failed to write output file");
    }
  }
}

fn build_single_file(
  source_file_path: std::path::PathBuf,
  matches: &clap::ArgMatches,
) -> Option<String> {
  if !source_file_path.is_file() {
    println!("provided build path is not a file or is inaccessible");

    return None;
  }

  let file_contents = std::fs::read_to_string(source_file_path.clone());

  if file_contents.is_err() {
    println!("path does not exist or is inaccessible");

    return None;
  }

  let mut lexer = gecko::lexer::Lexer::new(file_contents.unwrap().chars().collect());
  let llvm_context = inkwell::context::Context::create();
  let llvm_module =
  // TODO: Prefer usage of `.file_prefix()` once it is stable.
    llvm_context.create_module(source_file_path.file_stem().unwrap().to_str().unwrap());

  // let mut pass_manager = gecko::pass_manager::PassManager::new();

  let mut llvm_lowering_pass =
    gecko::llvm_lowering_pass::LlvmLoweringPass::new(&llvm_context, llvm_module);

  // pass_manager.add_pass(Box::new(llvm_lowering_pass));

  lexer.read_char();

  let tokens = lexer.collect();

  if matches.is_present(ARG_LIST_TOKENS) {
    println!("tokens: {:?}\n\n", tokens);
  }

  let mut parser = gecko::parser::Parser::new(tokens);
  let package_result = parser.parse_package_decl();

  if package_result.is_err() {
    println!("@parsing: {}", package_result.err().unwrap());

    return None;
  }

  let mut package = package_result.unwrap();

  while !parser.is_eof() {
    let top_level_node_result = parser.parse_top_level_node();

    if top_level_node_result.is_err() {
      println!("@parsing: {}", top_level_node_result.err().unwrap());

      return None;
    }

    let top_level_node = top_level_node_result.unwrap();

    package
      .symbol_table
      .insert(find_top_level_node_name(&top_level_node), top_level_node);
  }

  let mut entry_point_check_pass = gecko::entry_point_check_pass::EntryPointCheckPass {};
  let mut type_check_pass = gecko::type_check_pass::TypeCheckPass {};
  let mut diagnostics = vec![];

  for top_level_node in package.symbol_table.values() {
    match top_level_node {
      gecko::node::AnyTopLevelNode::Function(function) => {
        let entry_point_check_result = entry_point_check_pass.visit_function(&function);

        if entry_point_check_result.is_err() {
          diagnostics.push(entry_point_check_result.err().unwrap());
        }

        let type_check_result = type_check_pass.visit_function(&function);

        if type_check_result.is_err() {
          diagnostics.push(type_check_result.err().unwrap());
        }
      }
      _ => {}
    }
  }

  if !diagnostics.is_empty() {
    let mut error_count = 0;

    for diagnostic in diagnostics {
      if diagnostic.severity == gecko::diagnostic::DiagnosticSeverity::Error
        || diagnostic.severity == gecko::diagnostic::DiagnosticSeverity::Internal
      {
        error_count += 1;
      }

      println!("{}", diagnostic);
    }

    if error_count > 0 {
      println!(
        "\n{} error(s) were found; skipping lowering step",
        error_count
      );

      return None;
    }
  }

  let package_visitation_result = llvm_lowering_pass.visit_package(&package);

  if package_visitation_result.is_err() {
    println!("@lowering: {}", package_visitation_result.err().unwrap());

    return None;
  }

  let llvm_ir = llvm_lowering_pass.llvm_module.print_to_string();

  Some(
    llvm_ir
      .to_str()
      .expect("failed to emit LLVM IR from module")
      .trim()
      .to_string(),
  )

  // pass_manager.run(&parse_result.ok().unwrap());
}

fn main() {
  let app = App::new("Grip")
    .version(clap::crate_version!())
    .author(clap::crate_authors!())
    .about("Package manager & command-line utility for the gecko programming language")
    .arg(
      Arg::with_name(ARG_FILE)
        .help("The file to process")
        .index(1),
    )
    .arg(
      Arg::with_name(ARG_LIST_TOKENS)
        .short("t")
        .long(ARG_LIST_TOKENS)
        .help("Display a list of the lexed tokens"),
    )
    .arg(
      Arg::with_name(ARG_PRINT_LLVM_IR)
        .short("i")
        .long(ARG_PRINT_LLVM_IR)
        .help("Print the resulting LLVM IR instead of producing an output file"),
    )
    .subcommand(
      SubCommand::with_name(ARG_BUILD).about("Build the project in the current directory"),
    )
    .subcommand(
      SubCommand::with_name(ARG_INIT)
        .about("Initialize a default package manifest file in the current directory"),
    );

  let matches = app.get_matches();

  if matches.subcommand_matches(ARG_INIT).is_some() {
    let manifest_file_path = std::path::Path::new(PATH_MANIFEST_FILE);

    if manifest_file_path.exists() {
      println!("manifest file already exists in this directory");

      return;
    }

    let default_package_manifest = toml::ser::to_string_pretty(&PackageManifest {
      name: String::from("project"),
      version: String::from("0.0.1"),
    });

    if default_package_manifest.is_err() {
      println!("failed to serialize default package manifest");
    } else if std::fs::write(manifest_file_path, default_package_manifest.unwrap()).is_err() {
      println!("failed to write default package manifest to file");
    }

    return;
  } else if matches.subcommand_matches(ARG_BUILD).is_some() {
    let manifest_file_contents = std::fs::read_to_string(PATH_MANIFEST_FILE);

    if manifest_file_contents.is_err() {
      println!("path to package manifest does not exist or is inaccessible; run `grip --init` to initialize a default one in the current directory");

      return;
    }

    let manifest_toml_result =
      toml::from_str::<PackageManifest>(manifest_file_contents.unwrap().as_str());

    if manifest_toml_result.is_err() {
      println!("package manifest is not valid TOML");

      return;
    }

    let manifest_toml = manifest_toml_result.unwrap();
    let source_directory_paths = std::fs::read_dir("src");

    if source_directory_paths.is_err() {
      println!("path to package source files does not exist or is inaccessible");

      return;
    }

    let mut output_file_path = std::path::PathBuf::from(manifest_toml.name.clone());

    output_file_path.set_extension(PATH_OUTPUT_FILE_EXTENSION);

    for path in source_directory_paths.unwrap() {
      let path = path.unwrap().path();

      if path.is_file() {
        let file_extension = path.extension();

        if file_extension.is_none() || file_extension.unwrap() != PATH_SOURCE_FILE_EXTENSION {
          continue;
        }

        println!("compiling: {}", path.display());

        let llvm_ir = build_single_file(path, &matches);

        if llvm_ir.is_none() {
          break;
        }

        write_or_print_output(llvm_ir.unwrap(), output_file_path.clone(), &matches)
      }
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

    if let Some(llvm_ir) = build_single_file(source_file_path.clone(), &matches) {
      let mut output_file_path = std::path::PathBuf::from(source_file_path.parent().unwrap());

      output_file_path.push(source_file_path.file_stem().unwrap());
      output_file_path.set_extension(PATH_OUTPUT_FILE_EXTENSION);

      write_or_print_output(llvm_ir, output_file_path, &matches);
    }
  }
}
