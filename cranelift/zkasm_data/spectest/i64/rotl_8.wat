(module
 (import "env" "assert_eq_i64" (func $assert_eq_i64 (param i64) (param i64)))
 (func $main
	i64.const 0xabd1234ef567809c
	i64.const 63
	i64.rotl
	i64.const 0x55e891a77ab3c04e
	call $assert_eq_i64)
 (export "main" (func $main)))
