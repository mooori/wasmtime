;;! target = "riscv64"
;;!
;;! settings = ['enable_heap_access_spectre_mitigation=true']
;;!
;;! compile = true
;;!
;;! [globals.vmctx]
;;! type = "i64"
;;! vmctx = true
;;!
;;! [globals.heap_base]
;;! type = "i64"
;;! load = { base = "vmctx", offset = 0, readonly = true }
;;!
;;! [globals.heap_bound]
;;! type = "i64"
;;! load = { base = "vmctx", offset = 8, readonly = true }
;;!
;;! [[heaps]]
;;! base = "heap_base"
;;! min_size = 0x10000
;;! offset_guard_size = 0xffffffff
;;! index_type = "i32"
;;! style = { kind = "dynamic", bound = "heap_bound" }

;; !!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!
;; !!! GENERATED BY 'make-load-store-tests.sh' DO NOT EDIT !!!
;; !!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!

(module
  (memory i32 1)

  (func (export "do_store") (param i32 i32)
    local.get 0
    local.get 1
    i32.store8 offset=0x1000)

  (func (export "do_load") (param i32) (result i32)
    local.get 0
    i32.load8_u offset=0x1000))

;; function u0:0:
;; block0:
;;   slli a0,a0,32
;;   srli a4,a0,32
;;   ld a3,8(a2)
;;   ugt a3,a4,a3##ty=i64
;;   ld a2,0(a2)
;;   add a2,a2,a4
;;   lui a4,1
;;   add a2,a2,a4
;;   li a4,0
;;   andi a3,a3,255
;;   sltu a3,zero,a3
;;   sub a5,zero,a3
;;   and a3,a4,a5
;;   not a4,a5
;;   and a5,a2,a4
;;   or a2,a3,a5
;;   sb a1,0(a2)
;;   j label1
;; block1:
;;   ret
;;
;; function u0:1:
;; block0:
;;   slli a0,a0,32
;;   srli a3,a0,32
;;   ld a2,8(a1)
;;   ugt a2,a3,a2##ty=i64
;;   ld a1,0(a1)
;;   add a1,a1,a3
;;   lui a3,1
;;   add a1,a1,a3
;;   li a3,0
;;   andi a2,a2,255
;;   sltu a4,zero,a2
;;   sub a5,zero,a4
;;   and a2,a3,a5
;;   not a3,a5
;;   and a5,a1,a3
;;   or a1,a2,a5
;;   lbu a0,0(a1)
;;   j label1
;; block1:
;;   ret
