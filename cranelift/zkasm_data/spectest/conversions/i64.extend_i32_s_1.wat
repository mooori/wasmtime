(module
 (import "env" "assert_eq" (func $assert_eq (param i64) (param i64)))
 (func $main
	i32.const 0
	i64.extend_i32_s
	i64.const 0
	call $assert_eq)
 (start $main))
