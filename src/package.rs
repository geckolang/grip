use gecko::pass::*;
use serde::{Deserialize, Serialize};

const PATH_MANIFEST_FILE: &str = "grip.toml";
const PATH_SOURCE_FILE_EXTENSION: &str = "ko";
// TODO: Use CLI arguments (with default values) for both sources and output directories.
pub const PATH_SOURCES_DIRECTORY: &str = "src";
pub const PATH_OUTPUT_DIRECTORY: &str = "build";
pub const PATH_OUTPUT_FILE_EXTENSION: &str = "ll";

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

pub fn init_package_manifest() {
  let manifest_file_path = std::path::Path::new(PATH_MANIFEST_FILE);

  if manifest_file_path.exists() {
    println!("manifest file already exists in this directory");

    return;
  }

  // TODO: Display error.
  std::fs::create_dir(PATH_SOURCES_DIRECTORY);
  std::fs::create_dir(PATH_OUTPUT_DIRECTORY);

  let default_package_manifest = toml::ser::to_string_pretty(&PackageManifest {
    name: String::from("project"),
    version: String::from("0.0.1"),
  });

  if let Err(error_message) = default_package_manifest {
    println!("{}", error_message);
  } else if let Err(error_message) =
    std::fs::write(manifest_file_path, default_package_manifest.unwrap())
  {
    println!("{}", error_message);
  }
}

pub fn fetch_source_file_contents(source_file_path: &std::path::PathBuf) -> Result<String, String> {
  if !source_file_path.is_file() {
    return Err(String::from(
      "provided build path is not a file or is inaccessible",
    ));
  }

  let source_file_contents = std::fs::read_to_string(source_file_path.clone());

  if source_file_contents.is_err() {
    return Err(String::from("path does not exist or is inaccessible"));
  }

  Ok(source_file_contents.unwrap())
}

// TODO: Consider returning a `Vec<diagnostic::Diagnostic>` containing the actual problem(s) encountered.
pub fn build_single_file<'a>(
  llvm_context: &'a inkwell::context::Context,
  llvm_module: &inkwell::module::Module<'a>,
  source_file_contents: &String,
  matches: &clap::ArgMatches,
) -> Result<(), gecko::diagnostic::Diagnostic> {
  let mut lexer = gecko::lexer::Lexer::new(source_file_contents.chars().collect());

  // let mut pass_manager = gecko::pass_manager::PassManager::new();

  let mut llvm_lowering_pass =
    gecko::llvm_lowering_pass::LlvmLoweringPass::new(&llvm_context, llvm_module);

  // pass_manager.add_pass(Box::new(llvm_lowering_pass));

  lexer.read_char();

  let tokens = lexer.collect();

  if matches.is_present(crate::ARG_LIST_TOKENS) {
    println!("tokens: {:?}\n\n", tokens);
  }

  let mut parser = gecko::parser::Parser::new(tokens);
  let mut module = parser.parse_module_decl()?;
  let mut type_check_pass = gecko::type_check_pass::TypeCheckPass;

  while !parser.is_eof() {
    let top_level_node = parser.parse_top_level_node()?;

    match &top_level_node {
      gecko::node::AnyTopLevelNode::Function(function) => type_check_pass.visit_function(&function),
      _ => Ok(()),
    }?;

    module
      .symbol_table
      .insert(find_top_level_node_name(&top_level_node), top_level_node);
  }

  let mut entry_point_check_pass = gecko::entry_point_check_pass::EntryPointCheckPass {};
  // TODO:
  // let mut diagnostics = vec![];

  // for top_level_node in module.symbol_table.values() {
  //   match top_level_node {
  //     gecko::node::AnyTopLevelNode::Function(function) => {
  //       let entry_point_check_result = entry_point_check_pass.visit_function(&function);

  //       if entry_point_check_result.is_err() {
  //         diagnostics.push(entry_point_check_result.err().unwrap());
  //       }

  //       let type_check_result = type_check_pass.visit_function(&function);

  //       if type_check_result.is_err() {
  //         diagnostics.push(type_check_result.err().unwrap());
  //       }
  //     }
  //     _ => {}
  //   }
  // }

  // if !diagnostics.is_empty() {
  //   let mut error_count = 0;

  //   for diagnostic in diagnostics {
  //     if diagnostic.severity == gecko::diagnostic::DiagnosticSeverity::Error
  //       || diagnostic.severity == gecko::diagnostic::DiagnosticSeverity::Internal
  //     {
  //       error_count += 1;
  //     }

  //     println!("{}", diagnostic);
  //   }

  //   if error_count > 0 {
  //     println!(
  //       "\n{} error(s) were found; skipping lowering step",
  //       error_count
  //     );

  //     return false;
  //   }
  // }

  llvm_lowering_pass.visit_module(&module)

  // pass_manager.run(&parse_result.ok().unwrap());
}

pub fn build_package<'a>(
  llvm_context: &'a inkwell::context::Context,
  matches: &clap::ArgMatches,
) -> Option<(inkwell::module::Module<'a>, std::path::PathBuf)> {
  let manifest_file_contents = std::fs::read_to_string(PATH_MANIFEST_FILE);

  if manifest_file_contents.is_err() {
    println!("path to package manifest does not exist or is inaccessible; run `grip --init` to initialize a default one in the current directory");

    return None;
  }

  let manifest_toml_result =
    toml::from_str::<PackageManifest>(manifest_file_contents.unwrap().as_str());

  if manifest_toml_result.is_err() {
    println!("package manifest is not valid TOML");

    return None;
  }

  let manifest_toml = manifest_toml_result.unwrap();
  let source_directory_paths = std::fs::read_dir(PATH_SOURCES_DIRECTORY);

  if source_directory_paths.is_err() {
    println!("path to package source files does not exist or is inaccessible");

    return None;
  }

  let llvm_module =
  // TODO: Prefer usage of `.file_prefix()` once it is stable.
    llvm_context.create_module(manifest_toml.name.as_str());

  for path in source_directory_paths.unwrap() {
    let path = path.unwrap().path();

    if path.is_file() {
      let file_extension = path.extension();

      if file_extension.is_none() || file_extension.unwrap() != PATH_SOURCE_FILE_EXTENSION {
        continue;
      }

      println!("compiling: {}", path.display());

      let source_file_contents_result = fetch_source_file_contents(&path);

      if source_file_contents_result.is_err() {
        println!("{}", source_file_contents_result.err().unwrap());

        return None;
      }

      let source_file_contents = source_file_contents_result.unwrap();

      // FIXME: Verify that `llvm_module.clone` retains the same module instance.
      if let Err(diagnostic) =
        build_single_file(&llvm_context, &llvm_module, &source_file_contents, &matches)
      {
        // TODO: Should not be called here.
        crate::print_diagnostic(
          vec![(&path.to_str().unwrap().to_string(), &source_file_contents)],
          &diagnostic,
        );

        return None;
      }
    }
  }

  let mut output_file_path = std::path::PathBuf::from(manifest_toml.name.clone());

  output_file_path.set_extension(PATH_OUTPUT_FILE_EXTENSION);

  Some((llvm_module, output_file_path))
}
