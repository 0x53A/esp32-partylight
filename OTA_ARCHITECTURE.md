# Bluetooth OTA Architecture Diagram

```
┌─────────────────────────────────────────────────────────────────┐
│                         Client Device                            │
│  (Web Browser / Python Script / Mobile App)                     │
│                                                                   │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │  Bluetooth Client                                         │  │
│  │  - Connect to device                                      │  │
│  │  - Send OTA commands                                      │  │
│  │  - Transfer firmware chunks                               │  │
│  │  - Monitor status                                         │  │
│  └──────────────────────────────────────────────────────────┘  │
└────────────────────────┬─────────────────────────────────────────┘
                         │ Bluetooth Low Energy (BLE)
                         │ GATT Protocol
                         ▼
┌─────────────────────────────────────────────────────────────────┐
│                       ESP32-S3 Device                            │
│                                                                   │
│  ┌──────────────────────────────────────────────────────────┐  │
│  │  BLE GATT Server (trouble-host)                          │  │
│  │                                                           │  │
│  │  ┌────────────────────┐  ┌─────────────────────────┐    │  │
│  │  │  Config Service    │  │  OTA Service            │    │  │
│  │  │  (existing)        │  │  (new)                  │    │  │
│  │  │                    │  │                         │    │  │
│  │  │  • config_version  │  │  • ota_control (W/R)   │    │  │
│  │  │  • config_data     │  │  • ota_data (W)        │    │  │
│  │  │                    │  │  • ota_status (R/N)    │    │  │
│  │  └────────────────────┘  └─────────────────────────┘    │  │
│  │                                                           │  │
│  └──────────────────────────┬────────────────────────────────┘  │
│                             │                                   │
│  ┌──────────────────────────▼────────────────────────────────┐  │
│  │  OTA Logic (esp-bootloader-ota)                          │  │
│  │  - begin_ota() → Initialize partition                    │  │
│  │  - write_ota_data() → Write chunks                       │  │
│  │  - commit_ota() → Finalize & reboot                      │  │
│  │  - abort_ota() → Cancel update                           │  │
│  └──────────────────────────┬────────────────────────────────┘  │
│                             │                                   │
│  ┌──────────────────────────▼────────────────────────────────┐  │
│  │  Flash Memory Partitions                                  │  │
│  │                                                            │  │
│  │  ┌──────────┐  ┌──────────┐  ┌──────────┐               │  │
│  │  │   nvs    │  │ phy_init │  │ otadata  │               │  │
│  │  │  24 KB   │  │   4 KB   │  │   8 KB   │               │  │
│  │  └──────────┘  └──────────┘  └──────────┘               │  │
│  │                                                            │  │
│  │  ┌────────────────────────────────────────────┐          │  │
│  │  │  OTA Partition 0 (ota_0)                   │ ◄────┐   │  │
│  │  │  1 MB                                       │      │   │  │
│  │  │  [Active firmware or update target]        │      │   │  │
│  │  └────────────────────────────────────────────┘      │   │  │
│  │                                                        │   │  │
│  │  ┌────────────────────────────────────────────┐      │   │  │
│  │  │  OTA Partition 1 (ota_1)                   │ ◄────┘   │  │
│  │  │  1 MB                                       │  Alternate│  │
│  │  │  [Backup firmware or update target]        │  boot    │  │
│  │  └────────────────────────────────────────────┘  slots   │  │
│  │                                                            │  │
│  └────────────────────────────────────────────────────────────┘  │
│                                                                   │
│  ┌────────────────────────────────────────────────────────────┐  │
│  │  Bootloader (on reboot)                                    │  │
│  │  - Reads otadata to determine active partition             │  │
│  │  - Boots from active partition                             │  │
│  │  - On boot failure: tries alternate, then rolls back       │  │
│  └────────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘

Update Flow:
═══════════

1. Client → Device: Write 0x01 to ota_control (BEGIN)
2. Device: Initializes inactive partition (ota_1 if running ota_0)
3. Client → Device: Write firmware chunks to ota_data (512 bytes max)
4. Device: Writes chunks to flash partition
5. Client: Monitors ota_status (0x01 = in progress)
6. Client → Device: Write 0x02 to ota_control (COMMIT)
7. Device: Marks partition as bootable, validates, then reboots
8. Bootloader: Boots from new partition
9. On success: New firmware runs
   On failure: Bootloader automatically rolls back to previous partition

Error Handling:
══════════════

┌─────────────────┐
│  Connection     │──► Automatic abort
│  Lost           │    Update cancelled
└─────────────────┘

┌─────────────────┐
│  Write Error    │──► ota_status = 0x03 (ERROR)
│                 │    Client can retry or abort
└─────────────────┘

┌─────────────────┐
│  Boot Failure   │──► Bootloader rollback
│  (new firmware) │    Previous firmware restored
└─────────────────┘

┌─────────────────┐
│  Manual Abort   │──► Write 0x03 to ota_control
│                 │    Cleans up partial update
└─────────────────┘
```

## Key Design Decisions

1. **Dual Partition Layout**: Ensures safe updates with automatic rollback
2. **BLE Transport**: No USB cable needed for updates
3. **Chunked Transfer**: 512-byte chunks for BLE compatibility
4. **Status Reporting**: Real-time progress via read/notify characteristic
5. **Automatic Cleanup**: Connection loss triggers abort
6. **Bootloader Safety**: Failed boots automatically roll back

## Security Boundaries

```
┌─────────────────────────────────────────────────────────────┐
│  ⚠️  Current Implementation (Development/Testing)           │
│                                                              │
│  ✗ No firmware signature verification                       │
│  ✗ No client authentication                                 │
│  ✗ No encrypted data transfer                               │
│  ✗ No anti-rollback protection                              │
│                                                              │
│  → Suitable for development and trusted environments only   │
└─────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────┐
│  ✓  Production Recommendations                              │
│                                                              │
│  • Add firmware signing with secure boot                    │
│  • Implement BLE pairing/bonding                            │
│  • Use encrypted characteristics                            │
│  • Add version checking and rollback protection             │
│  • Implement rate limiting and access control               │
└─────────────────────────────────────────────────────────────┘
```
