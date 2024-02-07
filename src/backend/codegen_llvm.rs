use std::collections::HashMap;

use crate::frontend::nodes;
use crate::frontend::parser::Operation;
use crate::util::flags::Flags;
use crate::compiler::{OUTPUT_FOLDER, FILE_EXT, ERR_STR};
use crate::internal_panic;

use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::builder::Builder;
use inkwell::passes::PassManager;
use inkwell::targets::{Target, InitializationConfig, CodeModel, RelocMode, TargetTriple, TargetMachine};
use inkwell::types::{BasicType, BasicTypeEnum};
use inkwell::AddressSpace;
use inkwell::OptimizationLevel;
use inkwell::values::BasicValueEnum;

use tracer::trace_call;
use crate::middleend::type_checker::Type;

macro_rules! is_int {
    ($typ:expr) => {
        $typ == Type::I32 || $typ == Type::I64 || $typ == Type::U32 || $typ == Type::U64 || $typ == Type::Usize
    };
}

macro_rules! assert_is_int {
    ($lhs:expr, $rhs:expr) => {
        // FIXME: Fix this crap
        debug_assert!(
            is_int!($lhs.get_type()) && is_int!($rhs.get_type()),
            "FIXME FIXME FIXME FIXME FIXME, ONLY ACCEPTING INTS IS BAD"
        );
    };
}

macro_rules! fill_function_lookup {
    ($codegen:ident, $function:ident, $name:ident) => {
        // Prepare parameter types
        let mut param_types = Vec::new();
        for param in &$function.parameters {
            param_types.push($codegen.codegen_type(&param.typ.typ).into());
        }
        // Prepare return type
        let function_type;
        if $function.return_type.typ == Type::None {
            let return_type = $codegen.context.void_type();
            function_type = return_type.fn_type(&param_types, false);
        } else {
            let return_type = $codegen.codegen_type(&$function.return_type.typ);
            function_type = return_type.fn_type(&param_types, false);
        }
        let _function = $codegen.module.add_function(&$name, function_type, None);
    };
}

macro_rules! codegen_function_header {
    ($codegen:ident, $function:ident, $name:ident) => {
        let llvm_func = $codegen.module.get_function(&$name).unwrap();
        let entry = $codegen.context.append_basic_block(llvm_func, "entry");
        $codegen.builder.position_at_end(entry);
        for (i, param) in $function.parameters.iter().enumerate() {
            let func_param = llvm_func.get_nth_param(i as u32).unwrap();
            func_param.set_name(&param.name);
            let param_alloc = $codegen.builder.build_alloca(func_param.get_type(), &param.name);
            $codegen.add_variable(&param.name, param_alloc.into());
            $codegen.builder.build_store(param_alloc, func_param);
        }
    };
}

struct ClassInfo {
    fields: Vec<(String, Type)>,
}

impl ClassInfo {
    fn new() -> Self {
        Self {
            fields: Vec::new(),
        }
    }

    fn add_field(&mut self, field_name: &str, typ: &Type) {
        for (name, _) in &self.fields {
            debug_assert!(name != field_name);
        }
        self.fields.push((field_name.to_string(), typ.clone()));
    }

    fn get_field_index(&self, field_name: &str) -> usize {
        for (i, (name, _)) in self.fields.iter().enumerate() {
            if name == field_name {
                return i;
            }
        }
        unreachable!("Field {} not found in struct!", field_name);
    }
}

struct SizeManager {
    structs: HashMap<String, ClassInfo>,
}

impl SizeManager {
    fn new() -> Self {
        Self {
            structs: HashMap::new(),
        }
    }

    fn add_struct(&mut self, struct_name: &str) {
        debug_assert!(self.structs.get(struct_name).is_none());
        self.structs.insert(struct_name.to_string(), ClassInfo::new());
    }

    fn add_field(
        &mut self,
        struct_name: &str,
        field_name: &str,
        typ: &Type,
    ) {
        self.structs
            .get_mut(struct_name)
            .expect("Struct is guaranteed to be in here, because we typechecked before!")
            .add_field(field_name, typ);
    }

    fn get_struct_info(&self, struct_name: &str) -> &ClassInfo {
        self.structs.get(struct_name).expect("Struct is guaranteed to be in here, because we typechecked before!")
    }
}

pub struct LLVMCodegen<'flags, 'ctx> {
    path: String,
    objname: String,
    exename: String,
    context: &'ctx Context,
    module: Module<'ctx>,
    builder: Builder<'ctx>,
    target_machine: TargetMachine,

    stack_scopes: Vec<HashMap<String, BasicValueEnum<'ctx>>>,
    struct_defs: HashMap<String, BasicTypeEnum<'ctx>>,

    sm: SizeManager,
    flags: &'flags Flags,
}
impl<'flags, 'ctx> LLVMCodegen<'flags, 'ctx> {
    pub fn new(flags: &'flags Flags, context: &'ctx Context) -> Self {
        let mut path = flags.input.to_owned();
        if path.contains('/') {
            path = path.split('/').last().unwrap().to_string();
        }
        if path.contains('\\') {
            path = path.split('\\').last().unwrap().to_string();
        }
        let mut filename = String::from(OUTPUT_FOLDER);
        filename.push_str(&path);
        let objname = filename.replace(FILE_EXT, ".obj");
        let exename = objname.replace(".obj", ".exe");
        Target::initialize_native(&InitializationConfig::default()).unwrap();
        let target_str = Self::get_target().unwrap();
        let target = Target::from_name("x86-64").unwrap();
        let target_triple = TargetTriple::create(&target_str);
        let target_machine = target.create_target_machine(
            &target_triple,
            "x86-64",
            "+avx2",
            OptimizationLevel::Default,
            RelocMode::Default,
            CodeModel::Default,
        ).unwrap();
        let module_name = flags.input.clone();
        let module = context.create_module(&module_name);
        module.set_triple(&target_triple);
        let builder = context.create_builder();
        Self {
            path,
            objname,
            exename,
            context,
            module,
            builder,
            target_machine,
            struct_defs: HashMap::new(),
            stack_scopes: Vec::new(),
            sm: SizeManager::new(),
            flags,
        }
    }

    fn get_target() -> Result<String, String> {
        println!("[INFO] Getting host target");
        let output = std::process::Command::new("rustc")
            .arg("-vV")
            .output()
            .expect("Failed to run rustc to get the host target");
        let output = std::str::from_utf8(&output.stdout).expect("`rustc -vV` didn't return utf8 output");

        let field = "host: ";
        let host = output
            .lines()
            .find(|l| l.starts_with(field))
            .map(|l| &l[field.len()..])
            .ok_or_else(|| {
                format!(
                    "`rustc -vV` didn't have a line for `{}`, got:\n{}",
                    field.trim(),
                    output
                )
            })?
            .to_string();
        println!("[INFO] Host target is {}", host);
        Ok(host)
    }

    #[trace_call(always)]
    fn fill_lookup(&mut self, file: &nodes::FileNode) -> Result<(), String> {
        // Sort structs by field dependency, so we can create them in the correct order for LLVM
        // If struct `Foo` needs struct `Bar`, then `Bar` needs to be created first
        // This means building a tree of dependencies, and then sorting by them
        let mut struct_dependencies = HashMap::new();
        for strukt in &file.structs {
            struct_dependencies.insert(strukt.name.clone(), Vec::new());
        }
        for strukt in &file.structs {
            for field in &strukt.fields {
                if let Type::Struct(struct_name) = &field.type_def.typ {
                    struct_dependencies.get_mut(struct_name).unwrap().push(strukt.name.clone());
                }
            }
        }
        let mut sorted_structs = Vec::new();
        let mut structs_to_process = Vec::new();
        for strukt in &file.structs {
            if struct_dependencies.get(&strukt.name).unwrap().len() == 0 {
                structs_to_process.push(strukt.name.clone());
            }
        }
        while structs_to_process.len() > 0 {
            let struct_name = structs_to_process.pop().unwrap();
            sorted_structs.push(struct_name.clone());
            for (name, dependencies) in &mut struct_dependencies {
                if dependencies.contains(&struct_name) {
                    dependencies.retain(|n| n != &struct_name);
                    if dependencies.len() == 0 {
                        structs_to_process.push(name.clone());
                    }
                }
            }
        }
        if sorted_structs.len() != file.structs.len() {
            internal_panic!("Could not sort structs by dependency!");
        }
        sorted_structs.reverse();
        let mut structs = Vec::new();
        for struct_name in sorted_structs {
            for strukt in &file.structs {
                if strukt.name == struct_name {
                    structs.push(strukt.clone());
                }
            }
        }

        for strukt in &structs {
            self.sm.add_struct(&strukt.name);
            let struct_struct_type_def = self.context.opaque_struct_type(&strukt.name);
            let fields = strukt.fields.iter().map(|field| {
                let typ = &field.type_def.typ;
                debug_assert!(*typ != Type::None);
                self.sm.add_field(&strukt.name, &field.name, typ);
                self.codegen_type(&field.type_def.typ)
            });
            struct_struct_type_def.set_body(&fields.collect::<Vec<BasicTypeEnum>>(), false);
            self.struct_defs.insert(strukt.name.clone(), struct_struct_type_def.into());
        }
        for strukt in &file.structs {
            for method in &strukt.methods {
                let name = format!("{}_{}", strukt.name, method.name);
                fill_function_lookup!(self, method, name);
            }
        }
        for function in &file.functions {
            let name = function.name.clone();
            fill_function_lookup!(self, function, name);
        }
        for external in &file.externs {
            let name = external.name.clone();
            fill_function_lookup!(self, external, name);
        }
        Ok(())
    }

    #[trace_call(extra)]
    fn enter_scope(&mut self) {
        self.stack_scopes.push(HashMap::new());
    }

    #[trace_call(extra)]
    fn exit_scope(&mut self) {
        debug_assert!(self.stack_scopes.len() > 0);
        self.stack_scopes.pop();
    }

    #[trace_call(extra)]
    fn add_variable(&mut self, name: &str, value: BasicValueEnum<'ctx>) {
        debug_assert!(self.stack_scopes.len() > 0);
        self.stack_scopes.last_mut().unwrap().insert(name.to_string(), value);
    }

    #[trace_call(extra)]
    fn known_variable(&self, name: &str) -> bool {
        for scope in &self.stack_scopes {
            if scope.get(name).is_some() {
                return true;
            }
        }
        false
    }

    #[trace_call(extra)]
    fn get_variable(&self, name: &str) -> BasicValueEnum<'ctx> {
        for scope in self.stack_scopes.iter().rev() {
            if let Some(value) = scope.get(name) {
                return *value;
            }
        }
        unreachable!("Variable {} not found!", name);
    }

    #[trace_call(always)]
    pub fn create_executable(&mut self, file: &nodes::FileNode) -> Result<(), String> {
        self.fill_lookup(file)?;
        self.codegen_file(file)
    }

    #[trace_call(always)]
    fn codegen_file(&mut self, file: &nodes::FileNode) -> Result<(), String> {
        for strukt in &file.structs {
            self.codegen_struct(strukt)?;
        }
        for function in &file.functions {
            self.codegen_function(function)?;
        }
        match self.module.verify() {
            Ok(_) => (),
            Err(e) => {
                self.module.print_to_stderr();
                internal_panic!(format!("Module verification failed: {}", e.to_string()))
            },
        }
        if self.flags.debug {
            self.module.print_to_stderr();
        }

        // TODO: We already have an opt-flag, so we should add optimizations accordingly
        //       instead of just adding all of them
        let pass_manager = PassManager::create(());
        pass_manager.add_promote_memory_to_register_pass();
        pass_manager.add_instruction_combining_pass();
        pass_manager.add_reassociate_pass();
        pass_manager.add_gvn_pass();
        pass_manager.run_on(&self.module);

        let path = std::path::Path::new(&self.objname);
        self.target_machine.write_to_file(
            &self.module,
            inkwell::targets::FileType::Object,
            &path,
        ).unwrap();
        println!("[INFO] Created {}", path.to_str().unwrap());
        let clang_output = std::process::Command::new("clang")
            .arg("-o")
            .arg(&self.exename)
            .arg(&self.objname)
            .output()
            .expect("Failed to execute clang!");
        if !clang_output.status.success() {
            println!("[ERROR] Clang failed!");
            println!("[ERROR] {}", String::from_utf8_lossy(&clang_output.stderr));
            return Err("Clang failed!".to_string());
        }
        println!("[INFO] Created {}", self.exename);
        Ok(())
    }

    #[trace_call(always)]
    fn codegen_extern(&mut self, external: &nodes::ExternNode) -> Result<(), String> {
        let mut param_types = Vec::new();
        for param in &external.parameters {
            param_types.push(self.codegen_type(&param.typ.typ).into());
        }
        let function_type = if external.return_type.typ == Type::None {
            let return_type = self.context.void_type();
            return_type.fn_type(&param_types, false)
        } else {
            let return_type = self.codegen_type(&external.return_type.typ);
            return_type.fn_type(&param_types, false)
        };
        self.module.add_function(&external.name, function_type, None);
        Ok(())
    }

    #[trace_call(always)]
    fn codegen_struct(&mut self, strukt: &nodes::StructNode) -> Result<(), String> {
        for method in &strukt.methods {
            self.codegen_method(method)?;
        }
        Ok(())
    }

    #[trace_call(always)]
    fn codegen_method(&mut self, method: &nodes::MethodNode) -> Result<(), String> {
        self.enter_scope();

        let name = format!("{}_{}", method.struct_name, method.name);
        codegen_function_header!(self, method, name);

        self.codegen_block(&method.block)?;

        if method.return_type.typ == Type::None && !method.block.llvm_has_terminator {
            self.builder.build_return(None);
        } else {
            // Method is guaranteed to return a value in block codegen
        }

        self.exit_scope();
        Ok(())
    }

    #[trace_call(always)]
    fn codegen_function(&mut self, function: &nodes::FunctionNode) -> Result<(), String> {
        self.enter_scope();

        let name = function.name.clone();
        codegen_function_header!(self, function, name);

        self.codegen_block(&function.block)?;

        if function.return_type.typ == Type::None && !function.block.llvm_has_terminator {
            self.builder.build_return(None);
        } else {
            // Function is guaranteed to return a value in block codegen
        }

        self.exit_scope();
        Ok(())
    }

    #[trace_call(always)]
    fn codegen_block(&mut self, block: &nodes::BlockNode) -> Result<(), String> {
        self.enter_scope();
        for statement in &block.statements {
            self.codegen_statement(statement)?;
        }
        self.exit_scope();
        Ok(())
        // todo!()
    }

    #[trace_call(always)]
    fn codegen_statement(&mut self, statement: &nodes::Statement) -> Result<(), String> {
        match statement {
            nodes::Statement::Block(block) => self.codegen_block(block),
            nodes::Statement::VarDecl(var_decl_node) => self.codegen_stmt_var_decl(var_decl_node),
            nodes::Statement::If(if_node) => self.codegen_stmt_if(if_node),
            nodes::Statement::While(while_node) => self.codegen_stmt_while(while_node),
            nodes::Statement::Return(return_node) => self.codegen_stmt_return(return_node),
            nodes::Statement::Expression(expr) => {
                let _ = self.codegen_expression(expr, true)?;
                Ok(())
            },
            e => unimplemented!("codegen_statement: {:?}", e),
        }
    }

    #[trace_call(always)]
    fn codegen_stmt_while(&mut self, while_node: &nodes::WhileNode) -> Result<(), String> {
        let function = self.builder.get_insert_block().unwrap().get_parent().unwrap();
        let while_cond = self.context.append_basic_block(function, "codegen_stmt_while_cond");
        self.builder.build_unconditional_branch(while_cond);
        self.builder.position_at_end(while_cond);
        let condition = self.codegen_expression(&while_node.condition, true)?;
        let condition = self.builder.build_int_compare(
            inkwell::IntPredicate::NE,
            condition.into_int_value(),
            self.context.bool_type().const_int(0, false),
            "codegen_stmt_while_cond",
        );
        let while_body = self.context.append_basic_block(function, "codegen_stmt_while_body");
        let while_after = self.context.append_basic_block(function, "codegen_stmt_while_after");
        self.builder.build_conditional_branch(condition, while_body, while_after);
        self.builder.position_at_end(while_body);
        self.codegen_block(&while_node.body)?;
        if !while_node.body.llvm_has_terminator {
            self.builder.build_unconditional_branch(while_cond);
        }
        self.builder.position_at_end(while_after);
        Ok(())
    }

    #[trace_call(always)]
    fn codegen_stmt_return(&mut self, return_node: &nodes::ReturnNode) -> Result<(), String> {
        match &return_node.return_value {
            Some(value) => {
                let value = self.codegen_expression(value, true)?;
                self.builder.build_return(Some(&value));
                Ok(())
            }
            None => {
                self.builder.build_return(None);
                Ok(())
            }
        }
    }

    #[trace_call(always)]
    fn codegen_stmt_var_decl(&mut self, let_node: &nodes::VarDeclNode) -> Result<(), String> {
        let typ = self.codegen_type(&let_node.typ.typ);
        let alloca = self.builder.build_alloca(typ, &let_node.name);
        self.add_variable(&let_node.name, alloca.into());
        let value = self.codegen_expression(&let_node.expression, true)?;
        self.builder.build_store(alloca, value);
        Ok(())
    }

    #[trace_call(always)]
    fn codegen_stmt_if(&mut self, if_node: &nodes::IfNode) -> Result<(), String> {
        let condition = self.codegen_expression(&if_node.condition, true)?;
        let condition = self.builder.build_int_compare(
            inkwell::IntPredicate::NE,
            condition.into_int_value(),
            self.context.bool_type().const_int(0, false),
            "codegen_stmt_if_cond",
        );

        let parent = self.builder.get_insert_block().unwrap().get_parent().unwrap();
        if let Some(else_body) = &if_node.else_body {
            let then_bb = self.context.append_basic_block(parent, "codegen_stmt_if_then");
            let else_bb = self.context.append_basic_block(parent, "codegen_stmt_if_else");

            self.builder.build_conditional_branch(condition, then_bb, else_bb);

            // Emit then block.
            self.builder.position_at_end(then_bb);
            self.codegen_block(&if_node.if_body)?;
            let then_bb = self.builder.get_insert_block().unwrap();

            // Emit else block.
            self.builder.position_at_end(else_bb);
            self.codegen_block(else_body)?;
            let else_bb = self.builder.get_insert_block().unwrap();
            match (if_node.if_body.llvm_has_terminator, else_body.llvm_has_terminator) {
                (true, true) => (),
                (true, false) => {
                    let cont_bb = self.context.append_basic_block(parent, "codegen_stmt_if_after");
                    self.builder.position_at_end(else_bb);
                    self.builder.build_unconditional_branch(cont_bb);
                    self.builder.position_at_end(cont_bb);
                },
                (false, true) => {
                    self.builder.position_at_end(then_bb);
                    let cont_bb = self.context.append_basic_block(parent, "codegen_stmt_if_after");
                    self.builder.build_unconditional_branch(cont_bb);
                    self.builder.position_at_end(cont_bb);
                },
                (false, false) => {
                    let cont_bb = self.context.append_basic_block(parent, "codegen_stmt_if_after");
                    self.builder.position_at_end(then_bb);
                    self.builder.build_unconditional_branch(cont_bb);
                    self.builder.position_at_end(else_bb);
                    self.builder.build_unconditional_branch(cont_bb);
                    self.builder.position_at_end(cont_bb);
                }
            }
            Ok(())
        } else {
            let then_bb = self.context.append_basic_block(parent, "codegen_stmt_if_then");
            let cont_bb = self.context.append_basic_block(parent, "codegen_stmt_if_after");

            self.builder.build_conditional_branch(condition, then_bb, cont_bb);

            // Emit then block.
            self.builder.position_at_end(then_bb);
            self.codegen_block(&if_node.if_body)?;
            if !if_node.if_body.llvm_has_terminator {
                self.builder.build_unconditional_branch(cont_bb);
            }

            // Emit merge block.
            self.builder.position_at_end(cont_bb);
            Ok(())
        }
    }

    #[trace_call(always)]
    fn codegen_expression(&mut self, expression: &nodes::Expression, load_var: bool) -> Result<BasicValueEnum<'ctx>, String> {
        match expression {
            nodes::Expression::Literal(literal) => self.codegen_literal(literal),
            nodes::Expression::Binary(binary) => self.codegen_binary(binary, load_var),
            nodes::Expression::Name(name) => self.codegen_name(name, load_var),
            nodes::Expression::FunctionCall(function_call) => self.codegen_function_call(function_call),
            nodes::Expression::Unary(unary) => self.codegen_unary(unary, load_var),
            nodes::Expression::StructLiteral(struct_literal) => self.codegen_struct_literal(struct_literal, load_var),
        }
    }

    #[trace_call(always)]
    fn codegen_unary(&mut self, unary_node: &nodes::UnaryNode, _load_var: bool) -> Result<BasicValueEnum<'ctx>, String> {
        match unary_node.operation {
            Operation::Negate => {
                let value = self.codegen_expression(&unary_node.expression, true)?;
                assert_is_int!(unary_node.expression, unary_node.expression);
                let result = self.builder.build_int_neg(value.into_int_value(), "codegen_unary_negate");
                Ok(result.into())
            }
            e => unimplemented!("codegen_unary: {:?}", e),
        }
    }

    #[trace_call(always)]
    fn codegen_struct_literal(&mut self, struct_literal: &nodes::StructLiteralNode, load_var: bool) -> Result<BasicValueEnum<'ctx>, String> {
        let mut expressions = Vec::new();
        for field in &struct_literal.fields {
            expressions.push(self.codegen_expression(&field.1, true)?);
        }
        let struct_type = self.struct_defs.get(&struct_literal.struct_name).unwrap();
        let struct_alloc = self.builder.build_alloca(*struct_type, "codegen_struct_literal");
        for (i, field) in struct_literal.fields.iter().enumerate() {
            let field_offset = self.sm.get_struct_info(&struct_literal.struct_name).get_field_index(&field.0);
            let field_alloc = self.builder.build_struct_gep(
                struct_type.clone(),
                struct_alloc,
                field_offset as u32,
                &field.0,
            ).unwrap();
            let value = expressions[i];
            self.builder.build_store(field_alloc, value);
        }
        if load_var {
            let struct_alloc = self.builder.build_load(
                self.codegen_type(&struct_literal.typ),
                struct_alloc,
                "codegen_struct_literal",
            );
            Ok(struct_alloc)
        } else {
            Ok(struct_alloc.into())
        }
    }

    #[trace_call(always)]
    fn codegen_function_call(&mut self, function_call: &nodes::CallNode) -> Result<BasicValueEnum<'ctx>, String> {
        let name = function_call.function_name.clone();
        if !self.module.get_function(&name).is_some() {
            internal_panic!(format!("Could not find function {}", name));
        }
        let function = self.module.get_function(&name).unwrap();
        let mut args = Vec::new();
        for arg in &function_call.arguments {
            args.push(self.codegen_expression(arg, true)?.into());
        }
        let result = self.builder.build_call(function, &args, "codegen_function_call");
        if result.try_as_basic_value().left().is_none() {
            // NOTE: The return value doesn't matter, the function returns None
            //       it's a workaround for `void_type` not being a BasicValueEnum
            Ok(self.context.i32_type().const_int(0, false).into())
        } else {
            Ok(result.try_as_basic_value().left().unwrap())
        }
    }

    #[trace_call(always)]
    fn codegen_name(&mut self, name: &nodes::NameNode, load_var: bool) -> Result<BasicValueEnum<'ctx>, String> {
        debug_assert!(self.known_variable(&name.name));
        let var = self.get_variable(&name.name);
        if load_var {
            let var = self.builder.build_load(
                self.codegen_type(&name.typ),
                var.into_pointer_value(),
                "codegen_name"
            );
            Ok(var)
        } else {
            Ok(var)
        }
    }

    #[trace_call(always)]
    fn codegen_binary(&mut self, binary: &nodes::BinaryNode, load_var: bool) -> Result<BasicValueEnum<'ctx>, String> {
        match &binary.operation {
            Operation::Add => {
                let lhs = self.codegen_expression(&binary.lhs, true)?;
                let rhs = self.codegen_expression(&binary.rhs, true)?;
                assert_is_int!(binary.lhs, binary.rhs);
                let result = self.builder.build_int_add(
                    lhs.into_int_value(),
                    rhs.into_int_value(),
                    "codegen_binary_add",
                );
                Ok(result.into())
            }
            Operation::Sub => {
                let lhs = self.codegen_expression(&binary.lhs, true)?;
                let rhs = self.codegen_expression(&binary.rhs, true)?;
                assert_is_int!(binary.lhs, binary.rhs);
                let result = self.builder.build_int_sub(
                    lhs.into_int_value(),
                    rhs.into_int_value(),
                    "codegen_binary_sub",
                );
                Ok(result.into())
            }
            Operation::Mul => {
                let lhs = self.codegen_expression(&binary.lhs, true)?;
                let rhs = self.codegen_expression(&binary.rhs, true)?;
                assert_is_int!(binary.lhs, binary.rhs);
                let result = self.builder.build_int_mul(
                    lhs.into_int_value(),
                    rhs.into_int_value(),
                    "codegen_binary_mul",
                );
                Ok(result.into())
            }
            Operation::Div => {
                let lhs = self.codegen_expression(&binary.lhs, true)?;
                let rhs = self.codegen_expression(&binary.rhs, true)?;
                assert_is_int!(binary.lhs, binary.rhs);
                let result = self.builder.build_int_signed_div(
                    lhs.into_int_value(),
                    rhs.into_int_value(),
                    "codegen_binary_div",
                );
                Ok(result.into())
            }
            Operation::Modulo => {
                let lhs = self.codegen_expression(&binary.lhs, true)?;
                let rhs = self.codegen_expression(&binary.rhs, true)?;
                assert_is_int!(binary.lhs, binary.rhs);
                let result = self.builder.build_int_signed_rem(
                    lhs.into_int_value(),
                    rhs.into_int_value(),
                    "codegen_binary_modulo",
                );
                Ok(result.into())
            }
            Operation::LessThan => {
                let lhs = self.codegen_expression(&binary.lhs, true)?;
                let rhs = self.codegen_expression(&binary.rhs, true)?;
                assert_is_int!(binary.lhs, binary.rhs);
                let result = self.builder.build_int_compare(
                    inkwell::IntPredicate::SLT,
                    lhs.into_int_value(),
                    rhs.into_int_value(),
                    "codegen_binary_lessthan",
                );
                Ok(result.into())
            }
            Operation::GreaterThan => {
                let lhs = self.codegen_expression(&binary.lhs, true)?;
                let rhs = self.codegen_expression(&binary.rhs, true)?;
                assert_is_int!(binary.lhs, binary.rhs);
                let result = self.builder.build_int_compare(
                    inkwell::IntPredicate::SGT,
                    lhs.into_int_value(),
                    rhs.into_int_value(),
                    "codegen_binary_greaterthan",
                );
                Ok(result.into())
            }
            Operation::LessThanOrEqual => {
                let lhs = self.codegen_expression(&binary.lhs, true)?;
                let rhs = self.codegen_expression(&binary.rhs, true)?;
                assert_is_int!(binary.lhs, binary.rhs);
                let result = self.builder.build_int_compare(
                    inkwell::IntPredicate::SLE,
                    lhs.into_int_value(),
                    rhs.into_int_value(),
                    "codegen_binary_lessthanequal",
                );
                Ok(result.into())
            }
            Operation::GreaterThanOrEqual => {
                let lhs = self.codegen_expression(&binary.lhs, true)?;
                let rhs = self.codegen_expression(&binary.rhs, true)?;
                assert_is_int!(binary.lhs, binary.rhs);
                let result = self.builder.build_int_compare(
                    inkwell::IntPredicate::SGE,
                    lhs.into_int_value(),
                    rhs.into_int_value(),
                    "codegen_binary_greaterthanequal",
                );
                Ok(result.into())
            }
            Operation::Equal => {
                let lhs = self.codegen_expression(&binary.lhs, true)?;
                let rhs = self.codegen_expression(&binary.rhs, true)?;
                let result = self.builder.build_int_compare(
                    inkwell::IntPredicate::EQ,
                    lhs.into_int_value(),
                    rhs.into_int_value(),
                    "codegen_binary_equal",
                );
                Ok(result.into())
            }
            Operation::NotEqual => {
                let lhs = self.codegen_expression(&binary.lhs, true)?;
                let rhs = self.codegen_expression(&binary.rhs, true)?;
                let result = self.builder.build_int_compare(
                    inkwell::IntPredicate::NE,
                    lhs.into_int_value(),
                    rhs.into_int_value(),
                    "codegen_binary_notequal",
                );
                Ok(result.into())
            }
            Operation::BitwiseAnd => {
                let lhs = self.codegen_expression(&binary.lhs, true)?;
                let rhs = self.codegen_expression(&binary.rhs, true)?;
                let result = self.builder.build_and(
                    lhs.into_int_value(),
                    rhs.into_int_value(),
                    "codegen_binary_and",
                );
                Ok(result.into())
            }
            Operation::BitwiseOr => {
                let lhs = self.codegen_expression(&binary.lhs, true)?;
                let rhs = self.codegen_expression(&binary.rhs, true)?;
                let result = self.builder.build_or(
                    lhs.into_int_value(),
                    rhs.into_int_value(),
                    "codegen_binary_or",
                );
                Ok(result.into())
            }
            Operation::BitwiseXor => {
                let lhs = self.codegen_expression(&binary.lhs, true)?;
                let rhs = self.codegen_expression(&binary.rhs, true)?;
                let result = self.builder.build_xor(
                    lhs.into_int_value(),
                    rhs.into_int_value(),
                    "codegen_binary_xor",
                );
                Ok(result.into())
            }
            Operation::Assign => {
                let var = self.codegen_expression(&binary.lhs, false)?;
                let value = self.codegen_expression(&binary.rhs, true)?;
                self.builder.build_store(var.into_pointer_value(), value);
                Ok(value)
            }
            Operation::Dot => {
                debug_assert!((*binary.lhs).get_type().is_struct());
                let lhs = self.codegen_expression(&binary.lhs, false)?;
                // FIXME: This is a hack to guarantee that we have a pointer to the struct
                //        This is very bad for performance, but it's the only way I could get it to work :^)
                let lhs = if !lhs.is_pointer_value() {
                    let new_lhs = self.builder.build_alloca(
                        self.codegen_type(&binary.lhs.get_type()),
                        "lhs_alloc",
                    );
                    self.builder.build_store(new_lhs, lhs);
                    new_lhs
                } else {
                    lhs.into_pointer_value()
                };

                let struct_name = (*binary.lhs).get_type().get_struct_name();
                let struct_struct_type = self.struct_defs.get(&struct_name).unwrap();
                match &*binary.rhs {
                    nodes::Expression::Name(field) => {
                        let offset = self.sm
                            .get_struct_info(&struct_name)
                            .get_field_index(&field.name);
                        let field_ptr = self.builder.build_struct_gep(
                            struct_struct_type.clone(),
                            lhs,
                            offset as u32,
                            "field_ptr").unwrap();
                        let field_type = self.codegen_type(&field.typ);
                        let field_ptr = self.builder.build_pointer_cast(
                            field_ptr,
                            field_type.ptr_type(AddressSpace::default()),
                            "field_ptr_cast",
                        );
                        if load_var {
                            let field_ptr = self.builder.build_load(
                                field_type,
                                field_ptr,
                                "field_ptr_load",
                            );
                            Ok(field_ptr)
                        } else {
                            Ok(field_ptr.into())
                        }
                    }
                    nodes::Expression::FunctionCall(method_call) => {
                        let method_name = format!("{}_{}", struct_name, method_call.function_name);
                        if !self.module.get_function(&method_name).is_some() {
                            internal_panic!("Could not find function {}");
                        }
                        let method = self.module.get_function(&method_name).unwrap();
                        let mut args = Vec::new();
                        let lhs = self.builder.build_load(
                            self.codegen_type(&binary.lhs.get_type()),
                            lhs,
                            "lhs_load",
                        );
                        args.push(lhs.into());
                        for arg in &method_call.arguments {
                            args.push(self.codegen_expression(arg, true)?.into());
                        }
                        let result = self.builder.build_call(method, &args, "method_call");
                        if result.try_as_basic_value().left().is_none() {
                            Ok(lhs.into())
                        } else {
                            Ok(result.try_as_basic_value().left().unwrap())
                        }
                    }
                    e => internal_panic!(
                        format!("This should never happen: Found {:?} as rhs of Dot operation!", e)
                    ),
                }
            }
            e => unimplemented!("codegen_binary: {:?}", e),
        }
    }

    #[trace_call(always)]
    fn codegen_literal(&mut self, literal: &nodes::LiteralNode) -> Result<BasicValueEnum<'ctx>, String> {
        match &literal.typ {
            Type::Bool => {
                let value = if literal.value == "true" { 1 } else { 0 };
                let value = self.context.bool_type().const_int(value as u64, false);
                Ok(value.into())
            }
            Type::I32 => {
                let value = literal.value.parse::<i32>().unwrap();
                let value = self.context.i32_type().const_int(value as u64, false);
                Ok(value.into())
            }
            Type::I64 => {
                let value = literal.value.parse::<i64>().unwrap();
                let value = self.context.i64_type().const_int(value as u64, false);
                Ok(value.into())
            }
            Type::U32 => {
                let value = literal.value.parse::<u32>().unwrap();
                let value = self.context.i32_type().const_int(value as u64, false);
                Ok(value.into())
            }
            Type::U64 => {
                let value = literal.value.parse::<u64>().unwrap();
                let value = self.context.i64_type().const_int(value as u64, false);
                Ok(value.into())
            }
            Type::Usize => {
                let value = literal.value.parse::<usize>().unwrap();
                let value = self.context.i64_type().const_int(value as u64, false);
                Ok(value.into())
            }
            e => unimplemented!("codegen_literal: {:?}", e),
        }
    }

    #[trace_call(always)]
    fn codegen_type(&self, typ: &Type) -> BasicTypeEnum<'ctx> {
        match typ {
            Type::I32 => self.context.i32_type().as_basic_type_enum(),
            Type::I64 => self.context.i64_type().as_basic_type_enum(),
            Type::U32 => self.context.i32_type().as_basic_type_enum(),
            Type::U64 => self.context.i64_type().as_basic_type_enum(),
            Type::Usize => self.context.i64_type().as_basic_type_enum(),
            Type::Bool => self.context.bool_type().as_basic_type_enum(),
            Type::Struct(_) => {
                let struct_name = typ.get_struct_name();
                let struct_struct_type = self.struct_defs.get(&struct_name).unwrap();
                struct_struct_type.clone()
            },
            // Note: Void does not exist as BasicTypeEnum, so void functions are handled differently
            Type::None => unreachable!("Type::None should never be used!"),
            e => unimplemented!("codegen_type: {:?}", e),
        }
    }

    #[trace_call(always)]
    pub fn run(&mut self) -> Result<(), String> {
        println!("[INFO] Running {}", self.exename);
        let mut filename = String::from(OUTPUT_FOLDER);
        filename.push_str(&self.path);
        let path = std::path::Path::new(&self.exename);
        let output = std::process::Command::new(path)
            .output()
            .expect("Failed to execute program!");
        let exit_code = output.status.code().unwrap();
        println!("[INFO] BEGIN OUTPUT\n{}\n[INFO] END OUTPUT", String::from_utf8(output.stdout).unwrap());
        println!("[INFO] Program exited with code 0x{:X}.", exit_code);
        if exit_code != 0 {
            Err(format!(
                "{}: Code execution failed with code 0x{:X}.",
                ERR_STR, exit_code
            ))
        } else {
            Ok(())
        }
    }
}