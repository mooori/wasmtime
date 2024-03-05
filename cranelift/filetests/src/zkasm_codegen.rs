//! zkASM code generation

use std::collections::HashMap;
use std::sync::Arc;

use cranelift_codegen::data_value::DataValue;
use cranelift_codegen::entity::EntityRef;
use cranelift_codegen::ir::function::FunctionParameters;
use cranelift_codegen::ir::ExternalName;
use cranelift_codegen::ir::Function;
use cranelift_codegen::isa::{zkasm, IsaBuilder, TargetIsa};
use cranelift_codegen::settings::Configurable;
use cranelift_codegen::{settings, CodegenError, FinalizedMachReloc, FinalizedRelocTarget};
use cranelift_reader::Comparison;
use cranelift_reader::Invocation;
use cranelift_wasm::{translate_module, ZkasmEnvironment};

/// ISA specific settings for zkASM codegen.
#[derive(Default, Debug)]
pub struct ZkasmSettings {
    /// Instruments generated zkASM to trace executed instructions.
    pub emit_profiling_info: bool,
}

/// Generates zkASM for the provided `wasm_module`.
pub fn generate_zkasm(settings: &ZkasmSettings, wasm_module: &[u8]) -> String {
    let flag_builder = settings::builder();
    let mut isa_builder = zkasm::isa_builder("zkasm-unknown-unknown".parse().unwrap());
    handle_zkasm_settings(settings, &mut isa_builder);
    let isa = isa_builder
        .finish(settings::Flags::new(flag_builder))
        .unwrap();
    let mut zkasm_environ = ZkasmEnvironment::new(isa.frontend_config());
    translate_module(wasm_module, &mut zkasm_environ).unwrap();

    let mut program: Vec<String> = Vec::new();

    let (main_func_index, main_func_type) = zkasm_environ
        .info
        .functions
        .iter()
        .find(|&(_, exportable_func)| exportable_func.export_names.contains(&"main".to_string()))
        .expect("Must have a `main` function");

    let signature = zkasm_environ.func_env().func_sig(main_func_type.entity);
    assert!(signature.params.is_empty());
    assert!(signature.returns.is_empty());

    // TODO: Preamble should be generated by a linker and/or clift itself.
    program.append(&mut generate_preamble(
        main_func_index.index(),
        &zkasm_environ.info.global_inits,
        &zkasm_environ.info.data_inits,
    ));

    let num_func_imports = zkasm_environ.get_num_func_imports();
    let mut context = cranelift_codegen::Context::new();
    for (def_index, func) in zkasm_environ.info.function_bodies.iter() {
        let func_index = num_func_imports + def_index.index();
        program.push(format!("function_{}:", func_index));

        let mut mem = vec![];
        context.func = func.clone();
        let compiled_code = context
            .compile_and_emit(&*isa, &mut mem, &mut Default::default())
            .unwrap();
        let mut code_buffer = compiled_code.code_buffer().to_vec();
        fix_relocs(
            &mut code_buffer,
            &func.params,
            compiled_code.buffer.relocs(),
        );

        let code = std::str::from_utf8(&code_buffer).unwrap();
        let lines: Vec<&str> = code.lines().collect();
        let mut lines = optimize_labels(&lines, func_index);
        program.append(&mut lines);

        context.clear();
    }

    program.append(&mut generate_postamble());
    program.join("\n")
}

fn handle_zkasm_settings(
    settings: &ZkasmSettings,
    isa_builder: &mut IsaBuilder<Result<Arc<dyn TargetIsa>, CodegenError>>,
) {
    if settings.emit_profiling_info {
        isa_builder.enable("emit_profiling_info").unwrap();
    }
}

/// Generates a preamble.
pub fn generate_preamble(
    main_func_index: usize,
    globals: &[(cranelift_wasm::GlobalIndex, cranelift_wasm::GlobalInit)],
    data_segments: &[(u64, Vec<u8>)],
) -> Vec<String> {
    let mut program: Vec<String> = Vec::new();

    // Generate global variable definitions.
    for (key, _) in globals {
        program.push(format!("VAR GLOBAL global_{}", key.index()));
    }

    program.push("start:".to_string());
    for (key, init) in globals {
        match init {
            cranelift_wasm::GlobalInit::I32Const(v) => {
                // ZKASM stores constants in 2-complement form, so we need a cast to unsigned.
                program.push(format!(
                    "  {} :MSTORE(global_{})  ;; Global32({})",
                    *v as u32,
                    key.index(),
                    v
                ));
            }
            cranelift_wasm::GlobalInit::I64Const(v) => {
                // ZKASM stores constants in 2-complement form, so we need a cast to unsigned.
                program.push(format!(
                    "  {} :MSTORE(global_{})  ;; Global64({})",
                    *v as u64,
                    key.index(),
                    v
                ));
            }
            _ => unimplemented!("Global type is not supported"),
        }
    }

    // Generate const data segments definitions.
    for (offset, data) in data_segments {
        program.push(format!("  {} => E", offset / 8));
        // Each slot stores 8 consecutive u8 numbers, with earlier addresses stored in lower
        // bits.
        for (i, chunk) in data.chunks(8).enumerate() {
            let mut chunk_data = 0u64;
            for c in chunk.iter().rev() {
                chunk_data <<= 8;
                chunk_data |= *c as u64;
            }
            program.push(format!("  {chunk_data}n :MSTORE(MEM:E + {i})"));
        }
    }

    // The total amount of stack available on ZKASM processor is 2^16 of 8-byte words.
    // Stack memory is a separate region that is independent from the heap.
    program.push("  0xffff => SP".to_string());
    program.push("  zkPC + 2 => RR".to_string());
    program.push(format!("  :JMP(function_{})", main_func_index));
    program.push("  :JMP(finalizeExecution)".to_string());
    program
}

#[allow(dead_code)]
fn generate_postamble() -> Vec<String> {
    let mut program: Vec<String> = Vec::new();
    // In the prover, the program always runs for a fixed number of steps (e.g. 2^23), so we
    // need an infinite loop at the end of the program to fill the execution trace to the
    // expected number of steps.
    // In the future we might need to put zero in all registers here.
    program.push("finalizeExecution:".to_string());
    program.push("  ${beforeLast()}  :JMPN(finalizeExecution)".to_string());
    program.push("                   :JMP(start)".to_string());
    program.push("INCLUDE \"helpers/2-exp.zkasm\"".to_string());
    program
}

// TODO: Relocations should be generated by a linker and/or clift itself.
#[allow(dead_code)]
fn fix_relocs(
    code_buffer: &mut Vec<u8>,
    params: &FunctionParameters,
    relocs: &[FinalizedMachReloc],
) {
    let mut delta = 0i32;
    for reloc in relocs {
        let start = (reloc.offset as i32 + delta) as usize;
        let mut pos = start;
        while code_buffer[pos] != b'\n' {
            pos += 1;
            delta -= 1;
        }

        let code =
            if let FinalizedRelocTarget::ExternalName(ExternalName::User(name)) = reloc.target {
                let name = &params.user_named_funcs()[name];
                if name.index == 0 {
                    b"  B :ASSERT".to_vec()
                } else {
                    format!("  zkPC + 2 => RR\n  :JMP(function_{})", name.index)
                        .as_bytes()
                        .to_vec()
                }
            } else {
                b"  UNKNOWN".to_vec()
            };
        delta += code.len() as i32;

        code_buffer.splice(start..pos, code);
    }
}

// TODO: Labels optimization already happens in `MachBuffer`, we need to find a way to leverage
// it.
/// Label name is formatted as follows: <label_name>_<function_id>_<label_id>
/// Function id is unique through whole program while label id is unique only
/// inside given function.
/// Label name must begin from label_.
#[allow(dead_code)]
fn optimize_labels(code: &[&str], func_index: usize) -> Vec<String> {
    let mut label_definition: HashMap<String, usize> = HashMap::new();
    let mut label_uses: HashMap<String, Vec<usize>> = HashMap::new();
    let mut lines = Vec::new();
    for (index, line) in code.iter().enumerate() {
        let mut line = line.to_string();
        if line.starts_with(&"label_") {
            // Handles lines with a label marker, e.g.:
            //   <label_name>_XXX:
            let index_begin = line.rfind("_").expect("Failed to parse label index") + 1;
            let label_name: String = line[..line.len() - 1].to_string();
            line.insert_str(index_begin - 1, &format!("_{}", func_index));
            label_definition.insert(label_name, index);
        } else if line.contains(&"label_") {
            // Handles lines with a jump to label, e.g.:
            // A : JMPNZ(<label_name>_XXX)
            let pos = line.rfind(&"_").unwrap() + 1;
            let label_name = line[line
                .find("label_")
                .expect(&format!("Error parsing label line '{}'", line))
                ..line
                    .rfind(")")
                    .expect(&format!("Error parsing label line '{}'", line))]
                .to_string();
            line.insert_str(pos - 1, &format!("_{}", func_index));
            label_uses.entry(label_name).or_default().push(index);
        }
        lines.push(line);
    }

    let mut lines_to_delete = Vec::new();
    for (label, label_line) in label_definition {
        match label_uses.entry(label) {
            std::collections::hash_map::Entry::Occupied(uses) => {
                if uses.get().len() == 1 {
                    let use_line = uses.get()[0];
                    if use_line + 1 == label_line {
                        lines_to_delete.push(use_line);
                        lines_to_delete.push(label_line);
                    }
                }
            }
            std::collections::hash_map::Entry::Vacant(_) => {
                lines_to_delete.push(label_line);
            }
        }
    }
    lines_to_delete.sort();
    lines_to_delete.reverse();
    for index in lines_to_delete {
        lines.remove(index);
    }
    lines
}

// TODO: fix same label names in different functions
/// Compiles a clif function.
pub fn compile_clif_function(func: &Function) -> Vec<String> {
    let flag_builder = settings::builder();
    let isa_builder = zkasm::isa_builder("zkasm-unknown-unknown".parse().unwrap());
    let isa = isa_builder
        .finish(settings::Flags::new(flag_builder))
        .unwrap();
    let mut context = cranelift_codegen::Context::for_function(func.clone());
    let compiled_code = context
        .compile(isa.as_ref(), &mut Default::default())
        .unwrap();
    let mut code_buffer = compiled_code.code_buffer().to_vec();
    fix_relocs(
        &mut code_buffer,
        &func.params,
        compiled_code.buffer.relocs(),
    );
    let code = std::str::from_utf8(&code_buffer).unwrap();
    let mut lines: Vec<String> = code.lines().map(|s| s.to_string()).collect();
    // TODO: I believe it can be done more beautiful way
    let mut funcname = func.name.to_string();
    funcname.remove(0);
    funcname.push(':');
    let mut res = vec![funcname];
    res.append(&mut lines);
    res
}

// TODO: this function should be much rewrited,
// now it works for one very basic case:
// Simple progam which don't contain globals or some other speciefic preamble\postamble
// Program don't need helper functions (for example 2-exp.zkasm)
// How to fix it? Use generate_preamble and provide correct inputs for it.
/// Builds zkASM used in filetests.
pub fn build_test_zkasm(functions: Vec<Vec<String>>, invocations: Vec<Vec<String>>) -> String {
    // TODO: use generate_preamble to get preamble
    let preamble = "\
start:
  zkPC + 2 => RR
    :JMP(main)
    :JMP(finalizeExecution)";
    let mut main = vec![
        "main:".to_string(),
        "  SP - 1 => SP".to_string(),
        "  RR :MSTORE(SP)".to_string(),
    ];
    for invocation in invocations {
        main.extend(invocation);
    }
    main.push("  SP - 1 => SP".to_string());
    main.push("  :JMP(RR)".to_string());
    let mut postamble = generate_postamble();
    let mut program = vec![preamble.to_string()];
    program.append(&mut main);
    for foo in functions {
        program.extend(foo);
    }
    program.append(&mut postamble);
    program.join("\n")
}

/// Compiles a invocation.
pub fn compile_invocation(
    invoke: Invocation,
    compare: Comparison,
    expected: Vec<DataValue>,
) -> Vec<String> {
    // Here I assume that each "function" in zkasm gets it's arguments from first N registers
    // and put result in A.
    // TODO: should be more robust way to do it, we need somehow define inputs and outputs
    let mut res: Vec<String> = Default::default();
    let registers = vec!["A", "B", "C", "D", "E"];

    let args = invoke.args;
    let funcname = invoke.func;

    // TODO: here we should pay attention to type of DataValue (I64 or I32)
    for (idx, arg) in args.iter().enumerate() {
        res.push(format!("  {} => {}", arg, registers[idx]))
    }
    res.push(format!("    :JMP({})", funcname));
    // TODO: handle functions with multiple outputs
    res.push(format!("  {} => B", expected[0]));
    // TODO: replace with call to host function
    res.push(format!("  CALL AWESOME ASSERT ({})", compare));
    res
}
