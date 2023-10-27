use std::collections::{HashMap, VecDeque};
use crate::{checker::Type, parser::Location, codegen::{ERR_STR, NOTE_STR, WARN_STR}};

use super::nodes;

const BUILT_IN_FEATURES: [&str; 1] = ["new"];

#[derive(Debug, Clone)]
pub struct Parameter {
    name: String,
    location: Location,
    typ: Type
}

#[derive(Debug)]
pub struct Method {
    name: String,
    location: Location,
    return_type: Type,
    parameters: Vec<Parameter>,
}

impl Method {
    fn add_this_param(&mut self, class_type: &Type) -> Result<(), String> {
        // FIXME: Should this be part of the Parser?
        
        // FIXME: Calculate correct location for this param
        if self.parameters.len() == 0 {
            self.parameters.push(Parameter {
                name: String::from("this"),
                location: self.location.clone(),
                typ: class_type.clone()
            });
        } else {
            let first_param = &mut self.parameters[0];
            if first_param.name == String::from("this") {
                if first_param.typ != *class_type {
                    return Err(format!(
                        "{}: {:?}: Unexpected Type for parameter `this`.\n{}: `this` is an implicit parameter. Specifying it is not necessary.",
                        ERR_STR,
                        first_param.location,
                        NOTE_STR
                    ));
                }
                println!("{}: {:?}: Use of implicit parameter `this`.", WARN_STR, first_param.location);
                return Ok(());
            } else {
                for param in &self.parameters {
                    if param.name == String::from("this") {
                        return Err(format!(
                            "{}: {:?}: Unexpected Parameter. `this` is either implicit or the first parameter.",
                            ERR_STR,
                            param.location
                        ))
                    }
                }
            }
            // We have >0 parameters, none of them are named `this`
            self.parameters.insert(0, Parameter {
                name: String::from("this"),
                location: self.location.clone(),
                typ: class_type.clone()
            });
        }
        Ok(())
    }
}


#[derive(Debug)]
pub struct Function {
    name: String,
    location: Location,
    // FIXME: Also store return declaration for better error reporting
    return_type: Type,
    parameters: Vec<Parameter>,
}

#[derive(Debug)]
pub struct Class {
    name: String,
    class_type: Type,
    location: Location,
    fields: HashMap<String, (Location, Type)>,
    known_methods: HashMap<String, Method>,
    // FIXME: Maybe we should have an own struct for Features
    known_features: HashMap<String, Function>,
    has_constructor: bool,
}

impl Class {
    fn new(name: String) -> Self {
        Self {
            name: name.clone(),
            class_type: Type::Class(name),
            location: Location::anonymous(),
            fields: HashMap::new(),
            known_methods: HashMap::new(),
            known_features: HashMap::new(),
            has_constructor: false,
        }
    }

    fn add_field(&mut self, field: &nodes::FieldNode) -> Result<(), String> {
        let name = &field.name;
        let location = &field.location;
        
        match self.fields.get(name) {
            Some(f) => Err(format!(
                "{}: {:?}: Field redeclaration.\n{}: {:?}: Field already declared here.",
                ERR_STR,
                location,
                NOTE_STR,
                f.0
            )),
            None => {
                let typ = &field.type_def.typ;
                self.fields.insert(name.to_string(), (location.clone(), typ.clone()));
                Ok(())
            }
        }
    }

    fn add_method(&mut self, method: &nodes::MethodNode) -> Result<(), String> {
        let name = &method.name;
        let location = &method.location;
        match self.known_methods.get(name) {
            Some(f) => Err(format!(
                "{}: {:?}: Method redeclaration.\n{}: {:?}: Method already declared here.",
                ERR_STR,
                location,
                NOTE_STR,
                f.location
            )),
            None => {
                let return_type = &method.return_type.typ;
                let parameters: Vec<_> = method.parameters
                    .iter()
                    .map(|param| {
                        Parameter {
                            name: param.name.clone(),
                            location: param.location.clone(),
                            typ: param.typ.typ.clone()
                        }
                    })
                    .collect();
                let method = Method {
                    name: name.clone(),
                    location: location.clone(),
                    return_type: return_type.clone(),
                    parameters
                };
                self.known_methods.insert(name.clone(), method);
                Ok(())
            }
        }
    }

    fn add_feature(&mut self, feature: &nodes::FeatureNode) -> Result<(), String> {
        let name = &feature.name;
        let location = &feature.location;
        match self.known_features.get(name) {
            Some(f) => Err(format!(
                "{}: {:?}: Feature redeclaration.\n{}: {:?}: Feature already declared here.",
                ERR_STR,
                location,
                NOTE_STR,
                f.location
            )),
            None => {
                if !BUILT_IN_FEATURES.contains(&name.as_str()) {
                    // FIXME: Do we need to show all features? If yes, how do we display them nicely?
                    return Err(format!(
                        "{}: {:?}: Unknown feature `{}`.\n{}: This is a list of all features: {:?}",
                        ERR_STR,
                        location,
                        name,
                        NOTE_STR,
                        BUILT_IN_FEATURES
                    ));
                }
                let return_type = &feature.return_type.typ;
                let parameters: Vec<_> = feature.parameters
                    .iter()
                    .map(|param| {
                        Parameter {
                            name: param.name.clone(),
                            location: param.location.clone(),
                            typ: param.typ.typ.clone()
                        }
                    })
                    .collect();
                let func = Function {
                    name: name.clone(),
                    location: location.clone(),
                    return_type: return_type.clone(),
                    parameters
                };
                self.known_features.insert(name.clone(), func);
                Ok(())
            }
        }
    }

    fn resolve_new_return_type(&mut self) -> Result<(), String> {
        for feat in &mut self.known_features {
            if *feat.0 == String::from("new") {
                if feat.1.return_type == self.class_type {
                    // FIXME: Also store location of return type declaration for better warnings
                    println!(
                        "{}: {:?}: Use of implicit return type for constructor.",
                        WARN_STR,
                        feat.1.location
                    );
                    return Ok(());
                }
                if feat.1.return_type != Type::None {
                    return Err(format!(
                        "{}: {:?}: Feature `new` is expected to return None, found {}.",
                        ERR_STR,
                        feat.1.location,
                        feat.1.return_type
                    ));
                }
                feat.1.return_type = self.class_type.clone();
                return Ok(());
            }
        }
        assert!(false, "Class has constructor but no new feature found");
        unreachable!()
    }

    fn add_this_param(&mut self) -> Result<(), String> {
        // FIXME: Also add this parameter to features, once we have more of them
        for function in &mut self.known_methods {
            function.1.add_this_param(&self.class_type)?;
        }
        Ok(())
    }

    fn get_field(&self, name: &String) -> Option<(Location, Type)> {
        self.fields.get(name).cloned()
    }
}

#[derive(Debug, Clone)]
pub struct Variable {
    name: String,
    location: Location,
    typ: Type,
}

impl Variable {
    fn new(name: String, location: Location, typ: Type) -> Self {
        Self {
            name,
            location,
            typ
        }
    }
    fn is_class_instance(&self) -> bool {
        match &self.typ {
            Type::Class(..) => true,
            _ => false
        }
    }
    fn get_class_name(&self) -> String {
        match &self.typ {
            Type::Class(name) => name.clone(),
            _ => panic!()
        }
    }
}

#[derive(Debug)]
pub struct TypeChecker {
    known_functions: HashMap<String, Function>,
    known_classes: HashMap<String, Class>,
    known_variables: VecDeque<HashMap<String, Variable>>,
    current_function: String,
    current_class: String,
}

impl TypeChecker {
    pub fn new() -> Self {
        Self { 
            known_classes: HashMap::new(),
            known_functions: HashMap::new(),
            known_variables: VecDeque::new(),
            current_function: String::new(),
            current_class: String::new()
         }
    }

    fn add_function(&mut self, function: &nodes::FunctionNode) -> Result<(), String> {
        let name = &function.name;
        let location = &function.location;
        match self.known_functions.get(name) {
            Some(f) => Err(format!(
                "{}: {:?}: Function redeclaration.\n{}: {:?}: Function already declared here.",
                ERR_STR,
                location,
                NOTE_STR,
                f.location
            )),
            None => {
                let return_type = &function.return_type.typ;
                // FIXME: Check if parameter names repeat
                let parameters: Vec<_> = function.parameters
                .iter()
                .map(|param| {
                    Parameter {
                        name: param.name.clone(),
                        location: param.location.clone(),
                        typ: param.typ.typ.clone()
                    }
                })
                .collect();
                let func = Function {
                    name: name.clone(),
                    location: location.clone(),
                    return_type: return_type.clone(),
                    parameters
                };
                self.known_functions.insert(name.clone(), func);
                Ok(())
            }
        }
    }

    fn fill_lookup(&mut self, ast: &nodes::FileNode) -> Result<(), String> {
        for c in &ast.classes {
            match self.known_classes.get(&c.name) {
                Some(class) => return Err(format!(
                    "{}: {:?}: Class redeclaration.\n{}: {:?}: Class already declared here.",
                    ERR_STR,
                    c.location,
                    NOTE_STR,
                    class.location
                )),
                None => {
                    let mut class = Class::new(c.name.clone());
                    class.location = c.location.clone();
                    class.has_constructor = c.has_constructor;
                    for field in &c.fields {
                        class.add_field(field)?;
                    }
                    for method in &c.methods {
                        class.add_method(method)?;
                    }
                    for feature in &c.features {
                        class.add_feature(feature)?;
                    }
                    if class.has_constructor {
                        class.resolve_new_return_type()?;
                    }
                    class.add_this_param()?;
                    self.known_classes.insert(c.name.clone(), class);
                }
            }
        }
        for function in &ast.functions {
            self.add_function(function)?;
        }
        Ok(())
    }

    pub fn type_check_file(&mut self, ast: &mut nodes::FileNode) -> Result<Type, String> {
        self.fill_lookup(ast)?;
        ast.type_check(self)
    }

    fn add_scope(&mut self) {
        self.known_variables.push_back(HashMap::new());
    }

    fn remove_scope(&mut self) {
        self.known_variables.pop_back();
    }

    fn get_current_scope(&mut self) -> &mut HashMap<String, Variable> {
        let len = self.known_variables.len();
        &mut self.known_variables[len - 1]
    }

    fn get_variable(&self, name: &String) -> Option<Variable> {
        for scope in self.known_variables.iter().rev() {
            match scope.get(name) {
                Some(t) => return Some(t.clone()),
                None => ()
            }
        }
        None
    }
}

trait Typecheckable {
    fn type_check(&mut self, checker: &mut TypeChecker) -> Result<Type, String> where Self: Sized;
    fn type_check_with_type(&mut self, checker: &mut TypeChecker, typ: &Type) -> Result<(), String> where Self: Sized;
}

impl Typecheckable for nodes::FileNode {
    fn type_check(&mut self, checker: &mut TypeChecker) -> Result<Type, String> where Self: Sized {
        for c in &mut self.classes {
            c.type_check(checker)?;
        }
        for f in &mut self.functions {
            f.type_check(checker)?;
        }
        Ok(Type::None)
    }
    fn type_check_with_type(&mut self, checker: &mut TypeChecker, typ: &Type) -> Result<(), String> where Self: Sized {
        todo!()
    }
}
impl Typecheckable for nodes::ClassNode {
    fn type_check(&mut self, checker: &mut TypeChecker) -> Result<Type, String> where Self: Sized {
        debug_assert!(checker.current_class.is_empty());
        debug_assert!(checker.current_function.is_empty());
        checker.current_class = self.name.clone();
        // At this point, all known classes have entered the lookup
        // and all fields are unique
        // Only thing left to test is that the field types exist
        // and that functions and methods type check
        for field in &mut self.fields {
            field.type_check(checker)?;
        }
        for function in &mut self.methods {
            function.type_check(checker)?;
        }
        for feature in &mut self.features {
            feature.type_check(checker)?;
        }
        checker.current_class.clear();
        Ok(Type::None)
    }
    fn type_check_with_type(&mut self, checker: &mut TypeChecker, typ: &Type) -> Result<(), String> where Self: Sized {
        todo!()
    }
}
impl Typecheckable for nodes::FieldNode {
    fn type_check(&mut self, checker: &mut TypeChecker) -> Result<Type, String> where Self: Sized {
        self.type_def.type_check(checker)
    }
    fn type_check_with_type(&mut self, checker: &mut TypeChecker, typ: &Type) -> Result<(), String> where Self: Sized {
        todo!()
    }
}
impl Typecheckable for nodes::FieldAccess {
    fn type_check(&mut self, checker: &mut TypeChecker) -> Result<Type, String> where Self: Sized {
        todo!()
    }
    fn type_check_with_type(&mut self, checker: &mut TypeChecker, typ: &Type) -> Result<(), String> where Self: Sized {
        todo!()
    }
}
impl Typecheckable for nodes::FeatureNode {
    fn type_check(&mut self, checker: &mut TypeChecker) -> Result<Type, String> where Self: Sized {
        debug_assert!(checker.current_function.is_empty());
        debug_assert!(checker.known_variables.is_empty());
        debug_assert!(checker.known_classes.contains_key(&self.class_name));

        checker.current_function = self.name.clone();
        let Some(class_info) = checker.known_classes.get(&self.class_name) else { unreachable!() };
        // FIXME: Is the else actually unreachable for the feature?
        let Some(feature) = class_info.known_features.get(&self.name) else { unreachable!() };
        
        // Parameters are now known variables
        // FIXME: Parameter modification in fill_lookup should modify the AST too
        let mut parameters = HashMap::new();
        for param in &feature.parameters {
            let var = Variable::new(
                param.name.clone(),
                param.location.clone(),
                param.typ.clone()
            );
            match parameters.insert(param.name.clone(), var) {
                Some(param) => todo!(),
                None => ()
            }
        }
        checker.known_variables.push_back(parameters);

        // FIXME: Handle this better
        if feature.name == String::from("new") {
            let mut this_var = HashMap::new();
            this_var.insert(String::from("this"), Variable {
                name: String::from("this"),
                location: Location::anonymous(),
                typ: Type::Class(self.class_name.clone())
            });
            checker.known_variables.push_back(this_var);
        }
        
        self.block.type_check(checker)?;

        checker.current_function.clear();
        checker.known_variables.clear();
        Ok(Type::None)
    }
    fn type_check_with_type(&mut self, checker: &mut TypeChecker, typ: &Type) -> Result<(), String> where Self: Sized {
        todo!()
    }
}
impl Typecheckable for nodes::FunctionNode {
    fn type_check(&mut self, checker: &mut TypeChecker) -> Result<Type, String> where Self: Sized {
        debug_assert!(checker.current_function.is_empty());
        debug_assert!(checker.current_class.is_empty());
        debug_assert!(checker.known_variables.is_empty());
        debug_assert!(checker.known_functions.contains_key(&self.name));

        checker.current_function = self.name.clone();
        let Some(function) = checker.known_functions.get(&self.name) else { unreachable!() };

        let mut parameters = HashMap::new();
        for param in &function.parameters {
            let var = Variable::new(
                param.name.clone(),
                param.location.clone(),
                param.typ.clone()
            );
            match parameters.insert(param.name.clone(), var) {
                Some(param) => todo!(),
                None => ()
            }
        }
        checker.known_variables.push_back(parameters);

        self.block.type_check(checker)?;

        checker.current_function.clear();
        checker.known_variables.clear();
        Ok(Type::None)
    }
    fn type_check_with_type(&mut self, checker: &mut TypeChecker, typ: &Type) -> Result<(), String> where Self: Sized {
        todo!()
    }
}
impl Typecheckable for nodes::MethodNode {
    fn type_check(&mut self, checker: &mut TypeChecker) -> Result<Type, String> where Self: Sized {
        debug_assert!(checker.current_function.is_empty());
        debug_assert!(checker.known_variables.is_empty());
        debug_assert!(checker.known_classes.contains_key(&self.class_name));

        checker.current_function = self.name.clone();
        let Some(class_info) = checker.known_classes.get(&self.class_name) else { unreachable!() };
        // FIXME: Is the else actually unreachable for the method?
        let Some(method) = class_info.known_methods.get(&self.name) else { unreachable!() };
        
        // Parameters are now known variables
        // FIXME: Parameter modification in fill_lookup should modify the AST too
        let mut parameters = HashMap::new();
        for param in &method.parameters {
            let var = Variable::new(
                param.name.clone(),
                param.location.clone(),
                param.typ.clone()
            );
            match parameters.insert(param.name.clone(), var) {
                Some(param) => todo!(),
                None => ()
            }
        }
        checker.known_variables.push_back(parameters);
        
        self.block.type_check(checker)?;

        checker.current_function.clear();
        checker.known_variables.clear();
        Ok(Type::None)
    }
    fn type_check_with_type(&mut self, checker: &mut TypeChecker, typ: &Type) -> Result<(), String> where Self: Sized {
        todo!()
    }
}
impl Typecheckable for nodes::ParameterNode {
    fn type_check(&mut self, checker: &mut TypeChecker) -> Result<Type, String> where Self: Sized {
        todo!()
    }
    fn type_check_with_type(&mut self, checker: &mut TypeChecker, typ: &Type) -> Result<(), String> where Self: Sized {
        todo!()
    }
}
impl Typecheckable for nodes::BlockNode {
    fn type_check(&mut self, checker: &mut TypeChecker) -> Result<Type, String> where Self: Sized {
        checker.add_scope();
        for statement in &mut self.statements {
            statement.type_check(checker)?;
        }
        checker.remove_scope();
        Ok(Type::None)
    }
    fn type_check_with_type(&mut self, checker: &mut TypeChecker, typ: &Type) -> Result<(), String> where Self: Sized {
        todo!()
    }
}
impl Typecheckable for nodes::ExpressionNode {
    fn type_check(&mut self, checker: &mut TypeChecker) -> Result<Type, String> where Self: Sized {
        self.expression.type_check(checker)
    }
    fn type_check_with_type(&mut self, checker: &mut TypeChecker, typ: &Type) -> Result<(), String> where Self: Sized {
        self.expression.type_check_with_type(checker, typ)
    }
}
impl Typecheckable for nodes::Statement {
    fn type_check(&mut self, checker: &mut TypeChecker) -> Result<Type, String> where Self: Sized {
        match self {
            Self::Expression(expression) => expression.type_check(checker),
            Self::Let(let_node) => let_node.type_check(checker),
            Self::Assign(assignment) => assignment.type_check(checker),
            Self::If(if_node) => if_node.type_check(checker),
            Self::Return(return_node) => return_node.type_check(checker),
        }
    }
    fn type_check_with_type(&mut self, checker: &mut TypeChecker, typ: &Type) -> Result<(), String> where Self: Sized {
        todo!()
    }
}
impl Typecheckable for nodes::LetNode {
    fn type_check(&mut self, checker: &mut TypeChecker) -> Result<Type, String> where Self: Sized {
        match checker.get_variable(&self.name) {
            Some(var) => Err(format!(
                "{}: {:?}: Variable redeclaration.\n{}: {:?}: Variable `{}` already declared here.",
                ERR_STR,
                self.location,
                NOTE_STR,
                var.location,
                var.name
            )),
            None => {
                self.typ.type_check(checker)?;

                let current_scope = checker.get_current_scope();
                let var = Variable {
                    name: self.name.clone(),
                    location: self.location.clone(),
                    typ: self.typ.typ.clone()
                };
                debug_assert!(current_scope.insert(self.name.clone(), var).is_none());
                let expected_type = &self.typ.typ;

                let expr_type = self.expression.type_check(checker)?;
                debug_assert!(expr_type != Type::None);

                if expr_type == Type::Unknown {
                    // Couldnt determine type of expression
                    // We need to `infer` it
                    todo!()
                } else if expr_type != *expected_type {
                    Err(format!(
                        "{}: {:?}: Type Mismatch! Expected type `{:?}`, got type `{:?}`.",
                        ERR_STR,
                        self.location,
                        expected_type,
                        expr_type
                    ))
                } else {
                    Ok(expr_type)
                }
            }
        }
    }
    fn type_check_with_type(&mut self, checker: &mut TypeChecker, typ: &Type) -> Result<(), String> where Self: Sized {
        todo!()
    }
}
impl Typecheckable for nodes::AssignNode {
    fn type_check(&mut self, checker: &mut TypeChecker) -> Result<Type, String> where Self: Sized {
        let expected_type = self.name.type_check(checker)?;
        let rhs_type = self.expression.type_check(checker)?;
        if rhs_type == Type::Unknown {
            // We need to try and force the type of LHS to RHS
            self.expression.type_check_with_type(checker, &expected_type)?;
            Ok(rhs_type)
        } else if rhs_type != expected_type {
            Err(format!(
                "{}: {:?}: Can not assign `{}` to variable of type `{}`.",
                ERR_STR,
                self.expression.location,
                rhs_type,
                expected_type,
            ))
        } else {
            Ok(rhs_type)
        }
    }
    fn type_check_with_type(&mut self, checker: &mut TypeChecker, typ: &Type) -> Result<(), String> where Self: Sized {
        todo!()
    }
}
impl Typecheckable for nodes::IfNode {
    fn type_check(&mut self, checker: &mut TypeChecker) -> Result<Type, String> where Self: Sized {
        todo!()
    }
    fn type_check_with_type(&mut self, checker: &mut TypeChecker, typ: &Type) -> Result<(), String> where Self: Sized {
        todo!()
    }
}
impl Typecheckable for nodes::ReturnNode {
    fn type_check(&mut self, checker: &mut TypeChecker) -> Result<Type, String> where Self: Sized {
        debug_assert!(!checker.current_function.is_empty());
        // FIXME: Both branches share 80% same work, we can reduce code size and make this easier to read
        if checker.current_class.is_empty() {
            // We're returning from a normal function
            let Some(function) = checker.known_functions.get(&checker.current_function) else { unreachable!() };
            let expected_return_type = function.return_type.clone();
            let location = function.location.clone();
            debug_assert!(expected_return_type != Type::Unknown);

            if let Some(ret_expr) = &mut self.return_value {
                if expected_return_type == Type::None {
                    todo!() // FIXME: Unexpected return value
                }
                let expr_type = ret_expr.type_check(checker)?;
                if expr_type == Type::Unknown {
                    todo!() // FIXME: `infer` type
                } else if expr_type != expected_return_type {
                    Err(format!(
                        "{}: {:?}: Function is declared to return `{}`, found `{}`.\n{}: {:?}: Function declared to return `{}` here.",
                        NOTE_STR,
                        self.location,
                        expected_return_type,
                        expr_type,
                        NOTE_STR,
                        location,
                        expr_type
                    ))
                } else {
                    // Everything is fine, correct return type was provided
                    Ok(expr_type)
                }
            } else {
                // NOTE: This means no return value
                todo!()
            }
        } else {
            // We're returning from a method or feature
            let Some(class) = checker.known_classes.get(&checker.current_class) else { unreachable!() };
            let (mut location, mut expected_return_type) = (Location::anonymous(), Type::Unknown);
            match class.known_features.get(&checker.current_function) {
                Some(feature) => {
                    expected_return_type = feature.return_type.clone();
                    location = feature.location.clone();
                },
                None => ()
            }
            match class.known_methods.get(&checker.current_function) {
                Some(method) => {
                    expected_return_type = method.return_type.clone();
                    location = method.location.clone();
                }
                None => ()
            }
            debug_assert!(expected_return_type != Type::Unknown);
            debug_assert!(location != Location::anonymous());

            println!("{:?}", expected_return_type);
            if let Some(ret_expr) = &mut self.return_value {
                if expected_return_type == Type::None {
                    // Found expression but expected none, makes no sense
                    todo!()
                }
                let expr_type = ret_expr.type_check(checker)?;
                if expr_type == Type::Unknown {
                    // we have something like `return 5;`, where we couldn't determine the type
                    // so we now have to `infer` the type, and set it accordingly
                    todo!()
                } else if expr_type != expected_return_type {
                    // Signature expects `expected_return_type`, `return {expr}` has other type for expr
                    Err(format!(
                        "{}: {:?}: Function is declared to return `{}`, found `{}`.\n{}: {:?}: Function declared to return `{}` here.",
                        NOTE_STR,
                        self.location,
                        expected_return_type,
                        expr_type,
                        NOTE_STR,
                        location,
                        expr_type
                    ))
                } else {
                    // Everything is fine, correct return type was provided
                    Ok(expr_type)
                }
            } else {
                // NOTE: This means no return value
                todo!()
            }
        }
    }
    fn type_check_with_type(&mut self, checker: &mut TypeChecker, typ: &Type) -> Result<(), String> where Self: Sized {
        todo!()
    }
}
impl Typecheckable for nodes::TypeNode {
    fn type_check(&mut self, checker: &mut TypeChecker) -> Result<Type, String> where Self: Sized {
        match &self.typ {
            Type::Class(class_name) => {
                if !checker.known_classes.contains_key(class_name) {
                    Err(format!(
                        "{}: {:?}: Unknown Type `{}`.",
                        ERR_STR,
                        self.location,
                        self.typ
                    ))
                } else {
                    Ok(Type::None)
                }
            },
            _ => Ok(Type::None)
        }
    }
    fn type_check_with_type(&mut self, checker: &mut TypeChecker, typ: &Type) -> Result<(), String> where Self: Sized {
        todo!()
    }
}
impl Typecheckable for nodes::ArgumentNode {
    fn type_check(&mut self, checker: &mut TypeChecker) -> Result<Type, String> where Self: Sized {
        self.expression.type_check(checker)
    }
    fn type_check_with_type(&mut self, checker: &mut TypeChecker, typ: &Type) -> Result<(), String> where Self: Sized {
        self.expression.type_check_with_type(checker, typ)
    }
}
impl Typecheckable for nodes::Expression {
    fn type_check(&mut self, checker: &mut TypeChecker) -> Result<Type, String> where Self: Sized {
        match self {
            Self::Name(name_node) => name_node.type_check(checker),
            Self::Binary(binary_expr) => binary_expr.type_check(checker),
            Self::Identifier(ident_expr) => ident_expr.type_check(checker),
            Self::FunctionCall(func_call) => func_call.type_check(checker),
            Self::ConstructorCall(cons_call) => cons_call.type_check(checker),
            Self::BuiltIn(built_in) => built_in.type_check(checker),
            Self::ArrayAccess(access) => access.type_check(checker),
            Self::ArrayLiteral(literal) => literal.type_check(checker),
            Self::FieldAccess(access) => access.type_check(checker),
            Self::Literal(literal) => literal.type_check(checker),
            Self::None => unreachable!()
        }
    }
    fn type_check_with_type(&mut self, checker: &mut TypeChecker, typ: &Type) -> Result<(), String> where Self: Sized {
        match self {
            Self::Name(name_node) => name_node.type_check_with_type(checker, typ),
            Self::Binary(binary_expr) => binary_expr.type_check_with_type(checker, typ),
            Self::Identifier(ident_expr) => ident_expr.type_check_with_type(checker, typ),
            Self::FunctionCall(func_call) => func_call.type_check_with_type(checker, typ),
            Self::ConstructorCall(cons_call) => cons_call.type_check_with_type(checker, typ),
            Self::BuiltIn(built_in) => built_in.type_check_with_type(checker, typ),
            Self::ArrayAccess(access) => access.type_check_with_type(checker, typ),
            Self::ArrayLiteral(literal) => literal.type_check_with_type(checker, typ),
            Self::FieldAccess(access) => access.type_check_with_type(checker, typ),
            Self::Literal(literal) => literal.type_check_with_type(checker, typ),
            Self::None => unreachable!()
        }
    }
}
impl Typecheckable for nodes::ExpressionIdentifierNode {
    fn type_check(&mut self, checker: &mut TypeChecker) -> Result<Type, String> where Self: Sized {
        let typ = self.expression.type_check(checker)?;
        self.typ = typ.clone();
        Ok(typ)
    }
    fn type_check_with_type(&mut self, checker: &mut TypeChecker, typ: &Type) -> Result<(), String> where Self: Sized {
        todo!()
    }
}
impl Typecheckable for nodes::ExpressionArrayLiteralNode {
    fn type_check(&mut self, checker: &mut TypeChecker) -> Result<Type, String> where Self: Sized {
        todo!()
    }
    fn type_check_with_type(&mut self, checker: &mut TypeChecker, typ: &Type) -> Result<(), String> where Self: Sized {
        todo!()
    }
}
impl Typecheckable for nodes::ExpressionArrayAccessNode {
    fn type_check(&mut self, checker: &mut TypeChecker) -> Result<Type, String> where Self: Sized {
        todo!()
    }
    fn type_check_with_type(&mut self, checker: &mut TypeChecker, typ: &Type) -> Result<(), String> where Self: Sized {
        todo!()
    }
}
impl Typecheckable for nodes::ExpressionLiteralNode {
    fn type_check(&mut self, checker: &mut TypeChecker) -> Result<Type, String> where Self: Sized {
        Ok(self.typ.clone())
    }
    fn type_check_with_type(&mut self, checker: &mut TypeChecker, typ: &Type) -> Result<(), String> where Self: Sized {
        debug_assert!(self.typ == Type::Unknown);
        if let Type::Arr(_, _) = typ {
            todo!() // FIXME: Also generate error for: Attempted to infer array to literal.
        }
        if let Type::Class(class_name) = typ {
            // FIXME: How the heck do I deal with this error?
            // NOTE: We wanted to infer a class to a literal
            return Err(format!(
                "{}: {:?}: Unexpected type. Attempted to assign `{}` to literal `{}`.",
                ERR_STR,
                self.location,
                class_name,
                self.value
            ));
        }
        self.typ = typ.clone();
        Ok(())
    }
}
impl Typecheckable for nodes::ExpressionBinaryNode {
    fn type_check(&mut self, checker: &mut TypeChecker) -> Result<Type, String> where Self: Sized {
        let lhs_type = self.lhs.type_check(checker)?;
        let rhs_type = self.rhs.type_check(checker)?;
        println!("{:?} {:?}", lhs_type, rhs_type);
        debug_assert!(lhs_type != Type::None);
        debug_assert!(rhs_type != Type::None);
        // FIXME: Make this easier
        match (lhs_type, rhs_type) {
            (Type::Unknown, Type::Unknown) => {
                Ok(Type::Unknown)
            }
            (Type::Unknown, other) => {
                self.lhs.type_check_with_type(checker, &other)?;
                todo!();
            }
            (other, Type::Unknown) => {
                self.rhs.type_check_with_type(checker, &other)?;
                todo!()
            }
            (Type::Class(..), _) | (_, Type::Class(..))
            | (Type::Arr(..), _) | (_, Type::Arr(..)) => {
                todo!() // No operator overloading for classes and arrays yet
            }
            (lhs, rhs) => {
                if lhs != rhs {
                    // FIXME: Also store location of LHS and RHS for better error reporting
                    return Err(format!(
                        "{}: {:?}: Type Mismatch in binary expression. LHS has type `{:?}`, RHS has type `{:?}`.",
                        ERR_STR,
                        self.location,
                        lhs,
                        rhs
                    ));
                }
                self.typ = lhs.clone();
                Ok(lhs)
            }
        }
    }
    fn type_check_with_type(&mut self, checker: &mut TypeChecker, typ: &Type) -> Result<(), String> where Self: Sized {
        todo!()
    }
}
impl Typecheckable for nodes::ExpressionCallNode {
    fn type_check(&mut self, checker: &mut TypeChecker) -> Result<Type, String> where Self: Sized {
        let Some(function) = checker.known_functions.get(&self.function_name) else {
            return Err(format!(
                "{}: {:?}: Call to unknown function `{}`.",
                ERR_STR,
                self.location,
                self.function_name
            ));
        };
        let return_type = function.return_type.clone();
        match self.arguments.len().cmp(&function.parameters.len()) {
            std::cmp::Ordering::Less => {
                return Err(format!(
                    "{}: {:?}: Not enough arguments for call to function `{}`. Expected {} argument(s), found {}.\n{}: {:?}: Function declared here.",
                    ERR_STR,
                    self.location,
                    self.function_name,
                    function.parameters.len(),
                    self.arguments.len(),
                    NOTE_STR,
                    function.location
                ));
            }
            std::cmp::Ordering::Greater => {
                return Err(format!(
                    "{}: {:?}: Too manyarguments for call to function `{}`. Expected {} argument(s), found {}.\n{}: {:?}: Function declared here.",
                    ERR_STR,
                    self.location,
                    self.function_name,
                    function.parameters.len(),
                    self.arguments.len(),
                    NOTE_STR,
                    function.location
                ));
            }
            std::cmp::Ordering::Equal => ()
        }
        let params = function.parameters.clone();
        for (arg, param) in self.arguments.iter_mut().zip(params) {
            println!("{:#?}", arg);
            println!("{:#?}", param);
            let expected = param.typ;
            let arg_type = arg.type_check(checker)?;
            debug_assert!(arg_type != Type::None);
            if arg_type == Type::Unknown {
                // We need to `infer` the type again
                arg.type_check_with_type(checker, &expected)?;
            } else if arg_type != expected {
                return Err(format!(
                    "{}: {:?}: Type Mismatch in argument evaluation. Expected type `{:?}`, got type `{:?}`.",
                    ERR_STR,
                    arg.location,
                    expected,
                    arg_type
                ));
            } else {
                // Everything is cool
            }
        }
        self.typ = return_type;
        Ok(self.typ.clone())
    }
    fn type_check_with_type(&mut self, checker: &mut TypeChecker, typ: &Type) -> Result<(), String> where Self: Sized {
        todo!()
    }
}
impl Typecheckable for nodes::ExpressionConstructorNode {
    fn type_check(&mut self, checker: &mut TypeChecker) -> Result<Type, String> where Self: Sized {
        debug_assert!(!checker.current_function.is_empty());
        if !checker.known_classes.contains_key(&self.class_name) {
            return Err(format!(
                "{}: {:?}: Call to constructor of unknown class `{}`.\n{}: Capitalized function calls are always assumed to be constructor calls.",
                ERR_STR,
                self.location,
                self.class_name,
                NOTE_STR
            ))
        }
        debug_assert!(checker.known_classes.contains_key(&self.class_name));
        Ok(Type::Class(self.class_name.clone()))
    }
    fn type_check_with_type(&mut self, checker: &mut TypeChecker, typ: &Type) -> Result<(), String> where Self: Sized {
        todo!()
    }
}
impl Typecheckable for nodes::ExpressionFieldAccessNode {
    fn type_check(&mut self, checker: &mut TypeChecker) -> Result<Type, String> where Self: Sized {
        debug_assert!(!checker.current_function.is_empty());
        match checker.get_variable(&self.name) {
            Some(var) => {
                if !var.is_class_instance() {
                    return Err(format!(
                        "{}: {:?}: Variable `{}` is not a class instance, it has no fields.",
                        ERR_STR,
                        self.location,
                        self.name
                    ));
                }
                let typ = self.type_check_field(checker, var)?;
                self.typ = typ.clone();
                Ok(typ)
            }
            None => {
                Err(format!(
                    "{}: {:?}: Unknown variable `{}`.",
                    ERR_STR,
                    self.location,
                    self.name
                ))
            }
        }
    }
    fn type_check_with_type(&mut self, checker: &mut TypeChecker, typ: &Type) -> Result<(), String> where Self: Sized {
        todo!()
    }
}
impl nodes::ExpressionFieldAccessNode {
    fn type_check_field(&mut self, checker: &mut TypeChecker, var: Variable) -> Result<Type, String> {
        match &mut (*self.field.expression) {
            nodes::Expression::FieldAccess(field_access) => {
                /* 
                get type of current variable
                get fields of current variable, if its class (if not, error)
                check if field_access.name is field (if not, error)
                create temporary variable with name field_access.name and type field.typ
                recursively call type_check_field
                 */
                let typ = var.typ;
                match typ {
                    Type::Class(class_name) => {
                        let Some(class) = checker.known_classes.get(&class_name) else {
                            // NOTE: Actually, I think this might be unreachable...
                            todo!() // FIXME: Not a known class
                        };
                        let Some(field) = class.get_field(&field_access.name) else {
                            return Err(format!(
                                "{}: {:?}: Class `{}` has no field `{}`.\n{}: {:?}: Class declared here.",
                                ERR_STR,
                                field_access.location,
                                class.name,
                                field_access.name,
                                NOTE_STR,
                                class.location
                            ));
                        };
                        let var = Variable {
                            name: field_access.name.clone(),
                            location: field.0,
                            typ: field.1
                        };
                        let typ = field_access.type_check_field(checker, var.clone())?;
                        // FIXME: var.typ does not sound logical here, but it works for now
                        field_access.typ = var.typ.clone();
                        self.field.typ = var.typ.clone();
                        Ok(typ)
                    }
                    // NOTE: I think this might also be unreachable
                    _ => todo!() // FIXME: Not a class, doesnt have fields
                }
            },
            nodes::Expression::Name(name_node) => {
                let class_name = var.get_class_name();
                match checker.known_classes.get(&class_name) {
                    Some(class) => {
                        match class.get_field(&name_node.name) {
                            Some(field) => {
                                name_node.typ = field.1.clone();
                                self.field.typ = field.1.clone();
                                Ok(field.1)
                            },
                            None => Err(format!(
                                "{}: {:?}: Identifier `{}` has no field `{}`.",
                                ERR_STR,
                                self.location,
                                self.name,
                                name_node.name
                            ))
                        }
                    },
                    None => todo!()
                }
            },
            e => todo!("{:#?}", e)
        }
    }
}
impl Typecheckable for nodes::NameNode {
    fn type_check(&mut self, checker: &mut TypeChecker) -> Result<Type, String> where Self: Sized {
        match checker.get_variable(&self.name) {
            Some(var) => {
                self.typ = var.typ.clone();
                Ok(var.typ)
            },
            None => Err(format!(
                "{}: {:?}: Unknown variable `{}`.",
                ERR_STR,
                self.location,
                self.name
            ))
        }
    }
    fn type_check_with_type(&mut self, checker: &mut TypeChecker, typ: &Type) -> Result<(), String> where Self: Sized {
        todo!()
    }
}
impl Typecheckable for nodes::ExpressionBuiltInNode {
    fn type_check(&mut self, checker: &mut TypeChecker) -> Result<Type, String> where Self: Sized {
        todo!()
    }
    fn type_check_with_type(&mut self, checker: &mut TypeChecker, typ: &Type) -> Result<(), String> where Self: Sized {
        todo!()
    }
}