#!/usr/bin/env python3
"""
Bluetooth OTA Update Script for ESP32 Partylight

This script performs an Over-The-Air (OTA) firmware update via Bluetooth Low Energy.

Requirements:
    pip install bleak

Usage:
    python ota_update.py <firmware.bin> [device_name]

Example:
    python ota_update.py target/xtensa-esp32s3-none-elf/release/esp32_partylight.bin Blindomator
"""

import asyncio
import sys
import os
from bleak import BleakClient, BleakScanner
from pathlib import Path

# Service and characteristic UUIDs
OTA_SERVICE_UUID = "c6e7a9f0-1b34-4c5d-8f6e-2a3b4c5d6e7f"
OTA_CONTROL_CHAR_UUID = "d7f8b0e1-2c45-5d6e-9f7a-3b4c5d6e7f80"
OTA_DATA_CHAR_UUID = "e8f9c1d2-3d56-6e7f-a08b-4c5d6e7f8091"
OTA_STATUS_CHAR_UUID = "f9d0e2c3-4e67-7f80-b19c-5d6e7f809102"

# OTA commands
OTA_CMD_BEGIN = bytes([0x01])
OTA_CMD_COMMIT = bytes([0x02])
OTA_CMD_ABORT = bytes([0x03])

# OTA status codes
OTA_STATUS_IDLE = 0x00
OTA_STATUS_IN_PROGRESS = 0x01
OTA_STATUS_SUCCESS = 0x02
OTA_STATUS_ERROR = 0x03

# Configuration
CHUNK_SIZE = 512  # Maximum chunk size in bytes
CHUNK_DELAY = 0.05  # Delay between chunks in seconds


async def find_device(device_name=None):
    """Find the ESP32 device by name or service UUID."""
    print(f"Scanning for devices{f' with name {device_name}' if device_name else ''}...")
    
    devices = await BleakScanner.discover(timeout=5.0)
    
    for device in devices:
        # Match by name if provided
        if device_name and device.name and device_name.lower() in device.name.lower():
            print(f"Found device: {device.name} ({device.address})")
            return device.address
        
        # Or try to match by service UUID (requires connection)
        if not device_name:
            # For simplicity, just return the first device that looks like it could be our ESP32
            if device.name and ("blindomator" in device.name.lower() or "esp32" in device.name.lower()):
                print(f"Found device: {device.name} ({device.address})")
                return device.address
    
    raise RuntimeError(f"Device not found. Available devices: {[d.name for d in devices if d.name]}")


async def read_status(client):
    """Read the current OTA status."""
    status_bytes = await client.read_gatt_char(OTA_STATUS_CHAR_UUID)
    status = status_bytes[0] if len(status_bytes) > 0 else 0
    
    status_names = {
        OTA_STATUS_IDLE: "IDLE",
        OTA_STATUS_IN_PROGRESS: "IN_PROGRESS",
        OTA_STATUS_SUCCESS: "SUCCESS",
        OTA_STATUS_ERROR: "ERROR"
    }
    
    return status, status_names.get(status, f"UNKNOWN({status})")


async def perform_ota_update(address, firmware_path):
    """Perform the OTA update."""
    
    # Read firmware file
    firmware_path = Path(firmware_path)
    if not firmware_path.exists():
        raise FileNotFoundError(f"Firmware file not found: {firmware_path}")
    
    firmware_data = firmware_path.read_bytes()
    firmware_size = len(firmware_data)
    print(f"Firmware size: {firmware_size} bytes")
    
    async with BleakClient(address) as client:
        print(f"Connected to {address}")
        
        # Check if OTA service is available
        services = await client.get_services()
        ota_service = None
        for service in services:
            if service.uuid.lower() == OTA_SERVICE_UUID.lower():
                ota_service = service
                break
        
        if not ota_service:
            raise RuntimeError("OTA service not found. Device may not support OTA updates.")
        
        print("OTA service found")
        
        # Check initial status
        status, status_name = await read_status(client)
        print(f"Initial OTA status: {status_name}")
        
        if status == OTA_STATUS_IN_PROGRESS:
            print("WARNING: OTA already in progress. Aborting previous update...")
            await client.write_gatt_char(OTA_CONTROL_CHAR_UUID, OTA_CMD_ABORT)
            await asyncio.sleep(1)
        
        # Begin OTA update
        print("Beginning OTA update...")
        await client.write_gatt_char(OTA_CONTROL_CHAR_UUID, OTA_CMD_BEGIN)
        await asyncio.sleep(0.5)
        
        # Check status
        status, status_name = await read_status(client)
        print(f"OTA status after begin: {status_name}")
        
        if status == OTA_STATUS_ERROR:
            raise RuntimeError("Failed to begin OTA update")
        
        # Send firmware in chunks
        print(f"Sending firmware data ({firmware_size} bytes in {CHUNK_SIZE} byte chunks)...")
        chunks_sent = 0
        total_chunks = (firmware_size + CHUNK_SIZE - 1) // CHUNK_SIZE
        
        for offset in range(0, firmware_size, CHUNK_SIZE):
            chunk = firmware_data[offset:offset + CHUNK_SIZE]
            
            try:
                await client.write_gatt_char(OTA_DATA_CHAR_UUID, chunk)
                chunks_sent += 1
                
                # Progress indicator
                progress = (offset + len(chunk)) / firmware_size * 100
                print(f"Progress: {progress:.1f}% ({chunks_sent}/{total_chunks} chunks)", end='\r')
                
                # Small delay between chunks
                await asyncio.sleep(CHUNK_DELAY)
                
                # Periodically check status
                if chunks_sent % 20 == 0:
                    status, _ = await read_status(client)
                    if status == OTA_STATUS_ERROR:
                        raise RuntimeError(f"OTA error after {chunks_sent} chunks")
                
            except Exception as e:
                print(f"\nError writing chunk {chunks_sent}: {e}")
                print("Aborting OTA update...")
                await client.write_gatt_char(OTA_CONTROL_CHAR_UUID, OTA_CMD_ABORT)
                raise
        
        print(f"\nAll {chunks_sent} chunks sent successfully")
        
        # Commit the update
        print("Committing OTA update...")
        await client.write_gatt_char(OTA_CONTROL_CHAR_UUID, OTA_CMD_COMMIT)
        await asyncio.sleep(0.5)
        
        # The device should reboot after commit, so we might lose connection
        try:
            status, status_name = await read_status(client)
            print(f"Final OTA status: {status_name}")
            
            if status == OTA_STATUS_SUCCESS:
                print("OTA update successful! Device will reboot.")
            elif status == OTA_STATUS_ERROR:
                print("OTA update failed!")
                return False
        except Exception as e:
            # Connection lost is expected after reboot
            print(f"Connection lost (expected after reboot): {e}")
        
        print("OTA update completed successfully!")
        return True


async def main():
    if len(sys.argv) < 2:
        print(__doc__)
        sys.exit(1)
    
    firmware_path = sys.argv[1]
    device_name = sys.argv[2] if len(sys.argv) > 2 else None
    
    try:
        # Find device
        address = await find_device(device_name)
        
        # Perform OTA update
        success = await perform_ota_update(address, firmware_path)
        
        if success:
            print("\n✓ OTA update completed successfully!")
            sys.exit(0)
        else:
            print("\n✗ OTA update failed!")
            sys.exit(1)
            
    except Exception as e:
        print(f"\n✗ Error: {e}")
        import traceback
        traceback.print_exc()
        sys.exit(1)


if __name__ == "__main__":
    asyncio.run(main())
