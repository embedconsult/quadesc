/* Fixture only. Production firmware owns its linker layout. SRAM3 untouched. */
MEMORY {
  FLASH : ORIGIN = 0x00000000, LENGTH = 512K
  RAM : ORIGIN = 0x20000000, LENGTH = 96K
}
