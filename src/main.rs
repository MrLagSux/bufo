mod codegen;
mod lexer;
mod parser;

use std::process::exit;
use std::time::Instant;

use crate::codegen::Generator;
use crate::lexer::Lexer;
use crate::parser::Parser;

fn compile() -> Result<(), String> {
    let path = String::from("src/test.bu");

    let mut lexer = Lexer::new(&path)?;
    lexer.tokenize()?;
    // println!("{:#?}", lexer.get_tokens());

    let mut parser = Parser::new(lexer.get_tokens());
    let ast = parser.parse_file()?;
    // ast.print_debug();
    // exit(1);

    let mut generator = Generator::new(ast)?;
    // generator.compile()?;
    generator.interpret()?;
    Ok(())
}

fn main() {
    let now = Instant::now();
    if let Err(e) = compile() {
        println!("{}", e);
        exit(1);
    }
    println!("{:?}", now.elapsed());
}
