extern crate clap;
extern crate ionlang;

use clap::{App, Arg};
use ionlang::{llvm_lowering_pass, pass::*};

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
  let mut pass_manager = ionlang::pass_manager::PassManager::new();

  // TODO: Finish implementation.

  let mut llvm_lowering_pass =
    ionlang::llvm_lowering_pass::LlvmLoweringPass::new(&llvm_context, llvm_module);

  // pass_manager.add_pass(Box::new(llvm_lowering_pass));

  lexer.read_char();

  let tokens = lexer.collect();

  println!("Tokens: {:?}", tokens);

  let mut parser = ionlang::parser::Parser::new(tokens);
  let namespace_result = parser.parse_namespace();

  if namespace_result.is_err() {
    println!("parse_error: {:?}", namespace_result.err());
    return Err(String::from("failed to parse namespace"));
  }

  let visitation_result = llvm_lowering_pass.visit_namespace(&namespace_result.ok().unwrap());

  if visitation_result.is_err() {
    return Err(String::from(
      "visiting namespace yielded an error; module will not be emitted",
    ));
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
