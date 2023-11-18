
use crate::new::instr;

pub struct Assembler {
    print_debug: bool,
    path: String,
}

impl Assembler {
    pub fn new() -> Self {
        Self {
            print_debug: false,
            path: String::new()
        }
    }

    pub fn filepath(self, path: &String) -> Self {
        Self {
            path: path.clone(),
            ..self
        }
    }

    pub fn debug(self, print_debug: bool) -> Self {
        Self {
            print_debug,
            ..self
        }
    }

    pub fn generate_x86_64(&self, ir: Vec<instr::IR>) -> Result<(), String> {
        println!("{}", "-".repeat(50));
        let mut output = String::new();
        let mut push_asm = |s: &str| {
            output.push_str(s.clone());
            output.push('\n');
        };

        push_asm(format!("  ; Generated code for {}", self.path).as_str());
        push_asm("default rel");
        push_asm("");

        push_asm("segment .text");
        push_asm("  global main");
        push_asm("  extern ExitProcess");
        push_asm("  extern printf");
        push_asm("  extern malloc");
        push_asm("");

        for ir in &ir {
            if self.print_debug {
                push_asm(format!("; -- {ir:?} --").as_str());
            }
            use instr::IR;
            match ir {
                IR::Add { dst, src1, src2 } => {
                    debug_assert!(dst == src1);
                }
                IR::Label { name } => push_asm(format!("{name}:").as_str()),
                _ => push_asm("; Can't generate ASM for that yet")
            }
        }

        println!("{}", output);

        Ok(())
    }
}