(module
 (import "env" "assert_eq_i32" (func $assert_eq_i32 (param i32) (param i32)))
 (func $main
	i32.const 0x00080000
	i32.const 4
	i32.rotr
	i32.const 0x00008000
	call $assert_eq_i32)
 (export "main" (func $main)))
