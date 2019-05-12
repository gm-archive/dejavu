#![feature(optin_builtin_traits)]
#![feature(box_patterns)]
#![feature(box_syntax)]
#![feature(slice_patterns)]
#![feature(range_contains)]
#![feature(extern_types)]

use std::collections::HashMap;

use crate::symbol::Symbol;
use crate::front::{Lexer, Parser, ErrorHandler};
use crate::back::ssa;
use crate::vm::code;

pub use gml_meta::bind;

#[macro_use]
mod handle_map;
mod index_map;
mod bit_vec;
pub mod symbol;

pub mod front;
pub mod back;
pub mod vm;

/// A GML item definition, used as input to build a project.
pub enum Item<E> {
    Script(&'static str),
    Native(vm::ApiFunction<E>, usize, bool),
    Member(Option<vm::GetFunction<E>>, Option<vm::SetFunction<E>>),
}

/// Build a GML project.
pub fn build<E: Default, H: ErrorHandler, F: FnMut(Symbol, &str) -> H>(
    items: &HashMap<Symbol, Item<E>>,
    mut errors: F
) -> vm::Resources<E> {
    let prototypes: HashMap<Symbol, ssa::Prototype> = items.iter()
        .map(|(&name, resource)| match *resource {
            Item::Script(_) => (name, ssa::Prototype::Script),
            Item::Native(_, arity, variadic) => (name, ssa::Prototype::Native { arity, variadic }),
            Item::Member(_, _) => (name, ssa::Prototype::Member),
        })
        .collect();

    let mut resources = vm::Resources::default();
    for (&name, item) in items.iter() {
        match *item {
            Item::Script(source) => {
                let mut errors = errors(name, source);
                let (function, debug) = compile(&prototypes, source, &mut errors);
                resources.scripts.insert(name, function);
                resources.debug.insert(name, debug);
            }
            Item::Native(function, _, _) => {
                resources.api.insert(name, function);
            }
            Item::Member(get, set) => {
                if let Some(get) = get { resources.get.insert(name, get); }
                if let Some(set) = set { resources.set.insert(name, set); }
            }
        }
    }

    resources
}

fn compile(
    prototypes: &HashMap<Symbol, ssa::Prototype>, source: &str,
    errors: &mut dyn ErrorHandler
) -> (code::Function, code::Debug) {
    let reader = Lexer::new(source);
    let mut parser = Parser::new(reader, errors);
    let program = parser.parse_program();
    let codegen = front::Codegen::new(prototypes, errors);
    let program = codegen.compile(&program);
    let codegen = back::Codegen::new();
    codegen.compile(&program)
}
