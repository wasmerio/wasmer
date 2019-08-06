# (save_place, func(userdata), userdata)
.globl _unwinding_setjmp
_unwinding_setjmp:
push %r15
push %r14
push %r13
push %r12
push %rbx
push %rbp
sub $8, %rsp # 16-byte alignment
mov %rsp, (%rdi)
mov %rdx, %rdi
callq *%rsi
setjmp_ret:
add $8, %rsp
pop %rbp
pop %rbx
pop %r12
pop %r13
pop %r14
pop %r15
ret

.globl _unwinding_longjmp
_unwinding_longjmp:
mov %rdi, %rsp
jmp setjmp_ret
