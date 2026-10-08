/* Initial AM13E23019 envelope only. No persistence partition. */
MEMORY {
  FLASH : ORIGIN = 0x00000000, LENGTH = 512K
  RAM : ORIGIN = 0x20000000, LENGTH = 96K
}
