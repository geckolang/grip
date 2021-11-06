extern crate clap;
extern crate ionlang;

use clap::{App, Arg};
use ionlang::{llvm_lowering_pass, pass::*};

fn find_top_level_node_name(top_level_node: &ionlang::package::TopLevelNode) -> String {
  match top_level_node {
    ionlang::package::TopLevelNode::Function(function) => function.prototype.name.clone(),
    ionlang::package::TopLevelNode::External(external) => external.prototype.name.clone(),
  }
}

fn main() -> Result<(), String> {
  let matches = App::new("ilc")
    .version("1.0")
    .author("atlx")
    .about("Command-line interface for ionlang")
    .arg(
      Arg::with_name("file")
        .help("The file to process")
        .required(true)
        .index(1),
    )
    .get_matches();

  let file_contents = std::fs::read_to_string(matches.value_of("file").unwrap());

  if file_contents.is_err() {
    return Err(String::from("failed to read file path"));
  }

  let mut lexer = ionlang::lexer::Lexer::new(file_contents.unwrap().chars().collect());
  let llvm_context = inkwell::context::Context::create();
  let llvm_module = llvm_context.create_module("ilc");
  // let mut pass_manager = ionlang::pass_manager::PassManager::new();

  // TODO: Finish implementation.

  let mut llvm_lowering_pass =
    ionlang::llvm_lowering_pass::LlvmLoweringPass::new(&llvm_context, llvm_module);

  // pass_manager.add_pass(Box::new(llvm_lowering_pass));

  lexer.read_char();

  let tokens = lexer.collect();

  println!("Tokens: {:?}\n\n", tokens);

  let mut parser = ionlang::parser::Parser::new(tokens);
  let package_result = parser.parse_package_decl();

  if package_result.is_err() {
    println!("parse_error: {:?}", package_result.err());

    return Err(String::from("failed to parse package declaration"));
  }

  let mut package = package_result.unwrap();

  while !parser.is_eof() {
    let top_level_node_result = parser.parse_top_level_node();

    if top_level_node_result.is_err() {
      println!("parse_error: {:?}", top_level_node_result.err());

      return Err(String::from("failed to parse top-level construct"));
    }

    let top_level_node = top_level_node_result.unwrap();

    package
      .symbol_table
      .insert(find_top_level_node_name(&top_level_node), top_level_node);
  }

  let mut entry_point_check_pass = ionlang::entry_point_check_pass::EntryPointCheckPass {};
  let mut type_check_pass = ionlang::type_check_pass::TypeCheckPass {};

  for top_level_node in package.symbol_table.values() {
    match top_level_node {
      ionlang::package::TopLevelNode::Function(function) => {
        let entry_point_check_result = entry_point_check_pass.visit_function(&function);

        if entry_point_check_result.is_err() {
          println!(
            "entry_point_check_error: {:?}",
            entry_point_check_result.err()
          );

          return Err(String::from("failed to check entry point"));
        }

        let type_check_result = type_check_pass.visit_function(&function);

        if type_check_result.is_err() {
          println!("type_check_error: {:?}", type_check_result.err());

          return Err(format!(
            "failed to type-check function `{}`",
            function.prototype.name
          ));
        }
      }
      _ => {}
    }
  }

  let package_visitation_result = llvm_lowering_pass.visit_package(&package);

  if package_visitation_result.is_err() {
    println!("lowering_error: {:?}", package_visitation_result.err());

    return Err(String::from("there are errors; module will not be emitted"));
  }

  let llvm_ir = llvm_lowering_pass.llvm_module.print_to_string();

  println!(
    "==> LLVM IR: <==\n{}",
    llvm_ir
      .to_str()
      .expect("failed to emit LLVM IR from module")
  );

  // pass_manager.run(&parse_result.ok().unwrap());

  Ok(())
}
