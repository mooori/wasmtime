(module
 (import "env" "assert_eq_i32" (func $assert_eq_i32 (param i32) (param i32)))
 (func $main
	i32.const 5
	i32.const -2
	i32.rem_s
	i32.const 1
	call $assert_eq_i32)
 (export "main" (func $main)))
