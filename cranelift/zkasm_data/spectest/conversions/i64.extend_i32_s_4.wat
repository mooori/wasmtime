(module
 (import "env" "assert_eq" (func $assert_eq (param i64) (param i64)))
 (func $main
	i32.const -1
	i64.extend_i32_s
	i64.const -1
	call $assert_eq)
 (start $main))
