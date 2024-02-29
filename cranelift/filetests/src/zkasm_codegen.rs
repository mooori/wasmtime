use cranelift_codegen::data_value::DataValue;
use cranelift_codegen::entity::EntityRef;
use cranelift_codegen::ir::function::FunctionParameters;
use cranelift_codegen::ir::ExternalName;
use cranelift_codegen::ir::Function;
use cranelift_codegen::isa::zkasm;
use cranelift_codegen::{settings, FinalizedMachReloc, FinalizedRelocTarget};
use cranelift_reader::Comparison;
use cranelift_reader::Invocation;
use cranelift_wasm::{translate_module, ZkasmEnvironment};
use std::collections::HashMap;

#[allow(dead_code)]
pub fn generate_zkasm(wasm_module: &[u8]) -> String {
    let flag_builder = settings::builder();
    let isa_builder = zkasm::isa_builder("zkasm-unknown-unknown".parse().unwrap());
    let isa = isa_builder
        .finish(settings::Flags::new(flag_builder))
        .unwrap();
    let mut zkasm_environ = ZkasmEnvironment::new(isa.frontend_config());
    translate_module(wasm_module, &mut zkasm_environ).unwrap();

    let mut program: Vec<String> = Vec::new();

    let start_func = zkasm_environ
        .info
        .start_func
        .expect("Must have a start function");
    // TODO: Preamble should be generated by a linker and/or clift itself.
    program.append(&mut generate_preamble(
        start_func.index(),
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

#[allow(dead_code)]
pub fn generate_preamble(
    start_func_index: usize,
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
    program.push(format!("  :JMP(function_{})", start_func_index));
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
                    b"  $${assert_eq(A, B, label)}".to_vec()
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
    let mut res = vec![format!("{}:", funcname)];
    res.append(&mut lines);
    res.into_iter().map(|s| s.replace("label", &format!("label_{}", funcname))).collect()
}

pub fn build_main(invoke_names: Vec<String>) -> Vec<String> {
    let mut res = vec![
        "main:".to_string(),
        "  SP - 1 => SP".to_string(),
        "  RR :MSTORE(SP)".to_string(),
    ];
    for name in invoke_names {
        res.push("  zkPC + 2 => RR".to_string());
        res.push(format!("  :JMP({})", name));
    }
    res.push("  $ => RR :MLOAD(SP)".to_string());
    res.push("  SP + 1 => SP".to_string());
    res.push("  :JMP(RR)".to_string());
    res
}

pub fn invoke_name(invoke: &Invocation) -> String {
    let mut res = invoke.func.clone();
    for arg in &invoke.args {
        res.push_str(&format!("_{}", arg));
    }
    res
}

pub fn build_test_zkasm(
    functions: Vec<Vec<String>>,
    invocations: Vec<Vec<String>>,
    main: Vec<String>,
) -> String {
    // TODO: use generate_preamble to get preamble
    let preamble = "\
start:
  zkPC + 2 => RR
    :JMP(main)
    :JMP(finalizeExecution)";
    let mut postamble = generate_postamble();
    let mut program = vec![preamble.to_string()];
    program.extend(main);
    for inv in invocations {
        program.extend(inv);
    }
    for foo in functions {
        program.extend(foo);
    }
    program.append(&mut postamble);
    program.join("\n")
}

fn runcommand_to_wasm(
    invoke: Invocation,
    _compare: Comparison,
    expected: Vec<DataValue>,
) -> String {
    // TODO: support different amounts of outputs
    let res_bitness = match expected[0] {
        DataValue::I32(_) => "i32",
        DataValue::I64(_) => "i64",
        _ => unimplemented!(),
    };
    let func_name = invoke.func;
    let expected_result = expected[0].clone();
    let mut arg_types = String::new();
    let mut args_pushing = String::new();
    for arg in &invoke.args {
        let arg_type = match arg {
            DataValue::I32(_) => "i32",
            DataValue::I64(_) => "i64",
            _ => unimplemented!(),
        };
        arg_types.push_str(arg_type);
        arg_types.push_str(" ");

        args_pushing.push_str(&format!("{arg_type}.const {arg}\n        "));
    }
    if arg_types.len() > 0 {
        arg_types.pop();
    }
    // TODO: remove line with 8 whitespaces in the end of args_pushing
    let wat_code = format!(
        r#"(module
    (import "env" "assert_eq" (func $assert_eq (param {res_bitness} {res_bitness})))
    (import "env" "{func_name}" (func ${func_name} (param {arg_types}) (result {res_bitness})))
    (func $main
        {args_pushing}
        call ${func_name}
        {res_bitness}.const {expected_result}
        call $assert_eq
    )
    (start $main)
)"#,
        args_pushing = args_pushing,
        arg_types = arg_types,
        res_bitness = res_bitness,
        func_name = func_name,
        expected_result = expected_result,
    );
    wat_code.to_string()
}

pub fn compile_invocation(
    invoke: Invocation,
    compare: Comparison,
    expected: Vec<DataValue>,
) -> Vec<String> {
    // TODO: don't do this clones
    let cmp = if compare == Comparison::Equals {
        Comparison::Equals
    } else {
        Comparison::NotEquals
    };
    let inv = Invocation {
        func: invoke.func.clone(),
        args: invoke.args.clone(),
    };
    let wat = runcommand_to_wasm(inv, cmp, expected.clone());
    let wasm_module = wat::parse_str(wat).unwrap();

    // TODO: we should not use generate_zkasm itself, but a bit changed version.
    let generated: Vec<String> = generate_zkasm(&wasm_module)
        .split("\n")
        .map(|s| s.to_string())
        .collect();
    let new_label = invoke_name(&invoke);
    let funcname = invoke.func;

    let start_index = generated.iter().position(|r| r == "function_2:").unwrap();
    let end_index = generated.iter().rposition(|r| r == "  :JMP(RR)").unwrap();
    let mut generated_function = generated[start_index..=end_index].to_vec();
    
    generated_function[0] = format!("{}:", new_label);
    let generated_replaced: Vec<String> = generated_function
        .iter()
        .map(|s| {
            s.replace("label", &new_label)
                .replace("function_1", &funcname)
        })
        .collect();
    generated_replaced
}
