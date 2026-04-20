This project uses my Esp32-s3 microcontroller with a WeActStudio e-ink screen 400x300 screen.
Used certificates:
Digicert
Let's encrypt

## My device (Esp32-s3) config
BUSY - 15
RES - 4
D/C - 18
CS - 10
SCL - 12
SDA - 11

LED - 48
LED simple - 41

Esp32-s3 dev board memory layout
dram_seg = 0x3FCDB700 - 0x3FC88000 = 341760 bytes = 341 kb
dram2_seg = 0x3FCED710 - 0x3FCDB700 = 73774 bytes = 73 kb
rtc_fast_seg = 0x600fe000 ~ 0x60100000 = 8192 bytes = 8 kb
rtc_slow_seg = 0x50000000 ~ 0x50002000 = 8192 bytes = 8 kb
Mac: dc:da:0c:29:d3:c0
