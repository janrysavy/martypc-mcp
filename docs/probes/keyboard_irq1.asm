bits 16
cpu 8086
org 100h
cli
xor ax,ax
mov ds,ax
mov es,ax
mov ss,ax
mov sp,8000h
mov word [24h],180h
mov word [26h],0
mov al,13h
out 20h,al
mov al,8
out 21h,al
mov al,1
out 21h,al
mov al,0fdh
out 21h,al
mov al,40h
out 61h,al
sti
spin: jmp spin
times 180h-($-$$+100h) db 90h
push ax
push bx
push ds
xor ax,ax
mov ds,ax
mov bx,[280h]
in al,60h
mov [bx+300h],al
inc word [280h]
in al,61h
mov ah,al
or al,80h
out 61h,al
mov al,ah
out 61h,al
mov al,20h
out 20h,al
pop ds
pop bx
pop ax
iret
