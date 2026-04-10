[2-10]: Razer Blade USB Composite Device
|---USB Input Device
|   \---HID Keyboard Device
|---USB Input Device
|   |---Razer Blade 16
|   |---HID-compliant consumer control device
|   |---HID-compliant system controller
|   |---HID-compliant device
|   |---HID-compliant device
|   \---HID-compliant consumer control device
|       |---HID-compliant consumer control device
|       \---HID-compliant system controller
\---USB Input Device
    \---HID-compliant mouse
        \---Razer Control Device

    =========================== USB Port10 ===========================

Connection Status        : 0x01 (Device is connected)
Port Chain               : 2-10
Properties               : 0x00
 IsUserConnectable       : no
 PortIsDebugCapable      : no
 PortHasMultiCompanions  : no
 PortConnectorIsTypeC    : no
ConnectionIndex          : 0x0A (Port 10)

      ========================== Summary =========================
Vendor ID                : 0x1532 (Razer (Asia-Pacific) Pte Ltd.)
Product ID               : 0x029F
Manufacturer String      : "Razer"
Product String           : "Razer Blade"
Serial                   : ---
USB Version              : 2.0 (but 12 Mbit/s FullSpeed only)
Port maximum Speed       : High-Speed
Device maximum Speed     : Full-Speed
Device Connection Speed  : Full-Speed
Self powered             : no
Demanded Current         : 500 mA
Used Endpoints           : 4

      ======================== USB Device ========================

        +++++++++++++++++ Device Information ++++++++++++++++++
Device Description       : USB Composite Device
BusReported Device Desc  : Razer Blade
Device Path              : \\?\USB#VID_1532&PID_029F#5&35371215&0&10#{a5dcbf10-6530-11d2-901f-00c04fb951ed} (GUID_DEVINTERFACE_USB_DEVICE)
Kernel Name              : \Device\USBPDO-3
Device ID                : USB\VID_1532&PID_029F\5&35371215&0&10
Hardware IDs             : USB\VID_1532&PID_029F&REV_0200 USB\VID_1532&PID_029F
Driver KeyName           : {36fc9e60-c465-11cf-8056-444553540000}\0007 (GUID_DEVCLASS_USB)
Driver                   : \SystemRoot\System32\drivers\usbccgp.sys (Version: 10.0.26100.7344  Date: 2025-12-28  Company: Microsoft Corporation)
Driver Inf               : C:\WINDOWS\inf\usb.inf
Legacy BusType           : PNPBus
Class                    : USB
Class GUID               : {36fc9e60-c465-11cf-8056-444553540000} (GUID_DEVCLASS_USB)
Service                  : usbccgp
Enumerator               : USB
Location Info            : Port_#0010.Hub_#0001
Address                  : 10
Location IDs             : PCIROOT(0)#PCI(1400)#USBROOT(0)#USB(10), ACPI(_SB_)#ACPI(PC00)#ACPI(XHCI)#ACPI(RHUB)#ACPI(HS10)
Container ID             : {00000000-0000-0000-ffff-ffffffffffff} (GUID_CONTAINERID_INTERNALLY_CONNECTED_DEVICE)
Manufacturer Info        : (Standard USB Host Controller)
Capabilities             : 0x80 (SurpriseRemovalOK)
Status                   : 0x0180000A (DN_DRIVER_LOADED, DN_STARTED, DN_NT_ENUMERATOR, DN_NT_DRIVER)
First Install Date       : 2025-12-28 13:26:50
Last Arrival Date        : 2026-04-09 17:43:25
EnhancedPowerMgmtEnabled : 0
Power State              : D0 (supported: D0, D1, D2, D3, wake from D0, wake from D1, wake from D2)

        +++++++++++++++++ Registry USB Flags +++++++++++++++++
HKEY_LOCAL_MACHINE\SYSTEM\CurrentControlSet\Control\usbflags\1532029F0200
 osvc                    : REG_BINARY 01 01
 SkipContainerIdQuery    : REG_BINARY 01 00
 NewInterfaceUsage       : REG_DWORD 00000000 (0)

        ---------------- Connection Information ---------------
Connection Index         : 0x0A (Port 10)
Connection Status        : 0x01 (DeviceConnected)
Current Config Value     : 0x01 (Configuration 1)
Device Address           : 0x02 (2)
Is Hub                   : 0x00 (no)
Device Bus Speed         : 0x01 (Full-Speed)
Number of open Pipes     : 0x03 (3 pipes to data endpoints)
Pipe[0]                  : EndpointID=1  Direction=IN   ScheduleOffset=0  Type=Interrupt  wMaxPacketSize=0x8     bInterval=1   -> 196 Bits/ms = 24500 Bytes/s
Pipe[1]                  : EndpointID=2  Direction=IN   ScheduleOffset=0  Type=Interrupt  wMaxPacketSize=0x10    bInterval=1   -> 270 Bits/ms = 33750 Bytes/s
Pipe[2]                  : EndpointID=3  Direction=IN   ScheduleOffset=0  Type=Interrupt  wMaxPacketSize=0x8     bInterval=1   -> 196 Bits/ms = 24500 Bytes/s

        --------------- Connection Information V2 -------------
Connection Index         : 0x0A (10)
Length                   : 0x10 (16 bytes)
SupportedUsbProtocols    : 0x03
 Usb110                  : 1 (yes, port supports USB 1.1)
 Usb200                  : 1 (yes, port supports USB 2.0)
 Usb300                  : 0 (no, port not supports USB 3.0)
 ReservedMBZ             : 0x00
Flags                    : 0x00
 DevIsOpAtSsOrHigher     : 0 (Device is not operating at SuperSpeed or higher)
 DevIsSsCapOrHigher      : 0 (Device is not SuperSpeed capable or higher)
 DevIsOpAtSsPlusOrHigher : 0 (Device is not operating at SuperSpeedPlus or higher)
 DevIsSsPlusCapOrHigher  : 0 (Device is not SuperSpeedPlus capable or higher)
 ReservedMBZ             : 0x00

    ---------------------- Device Descriptor ----------------------
bLength                  : 0x12 (18 bytes)
bDescriptorType          : 0x01 (Device Descriptor)
bcdUSB                   : 0x200 (USB Version 2.0) -> but device is Full-Speed only
bDeviceClass             : 0x00 (defined by the interface descriptors)
bDeviceSubClass          : 0x00
bDeviceProtocol          : 0x00
bMaxPacketSize0          : 0x40 (64 bytes)
idVendor                 : 0x1532 (Razer (Asia-Pacific) Pte Ltd.)
idProduct                : 0x029F
bcdDevice                : 0x0200
iManufacturer            : 0x01 (String Descriptor 1)
 Language 0x0409         : "Razer"
iProduct                 : 0x02 (String Descriptor 2)
 Language 0x0409         : "Razer Blade"
iSerialNumber            : 0x00 (No String Descriptor)
bNumConfigurations       : 0x01 (1 Configuration)

    ------------------ Configuration Descriptor -------------------
bLength                  : 0x09 (9 bytes)
bDescriptorType          : 0x02 (Configuration Descriptor)
wTotalLength             : 0x0054 (84 bytes)
bNumInterfaces           : 0x03 (3 Interfaces)
bConfigurationValue      : 0x01 (Configuration 1)
iConfiguration           : 0x00 (No String Descriptor)
bmAttributes             : 0xA0
 D7: Reserved, set 1     : 0x01
 D6: Self Powered        : 0x00 (no)
 D5: Remote Wakeup       : 0x01 (yes)
 D4..0: Reserved, set 0  : 0x00
MaxPower                 : 0xFA (500 mA)

        ---------------- Interface Descriptor -----------------
bLength                  : 0x09 (9 bytes)
bDescriptorType          : 0x04 (Interface Descriptor)
bInterfaceNumber         : 0x00 (Interface 0)
bAlternateSetting        : 0x00
bNumEndpoints            : 0x01 (1 Endpoint)
bInterfaceClass          : 0x03 (HID - Human Interface Device)
bInterfaceSubClass       : 0x01 (Boot Interface)
bInterfaceProtocol       : 0x01 (Keyboard)
iInterface               : 0x00 (No String Descriptor)

        ------------------- HID Descriptor --------------------
bLength                  : 0x09 (9 bytes)
bDescriptorType          : 0x21 (HID Descriptor)
bcdHID                   : 0x0111 (HID Version 1.11)
bCountryCode             : 0x00 (00 = not localized)
bNumDescriptors          : 0x01
Descriptor 1:
bDescriptorType          : 0x22 (Class=Report)
wDescriptorLength        : 0x003D (61 bytes)
Error reading descriptor : ERROR_GEN_FAILURE (due to a obscure limitation of the Win32 USB API, see F1 Help)

        ----------------- Endpoint Descriptor -----------------
bLength                  : 0x07 (7 bytes)
bDescriptorType          : 0x05 (Endpoint Descriptor)
bEndpointAddress         : 0x81 (Direction=IN EndpointID=1)
bmAttributes             : 0x03 (TransferType=Interrupt)
wMaxPacketSize           : 0x0008 (8 bytes)
bInterval                : 0x01 (1 ms)

        ---------------- Interface Descriptor -----------------
bLength                  : 0x09 (9 bytes)
bDescriptorType          : 0x04 (Interface Descriptor)
bInterfaceNumber         : 0x01 (Interface 1)
bAlternateSetting        : 0x00
bNumEndpoints            : 0x01 (1 Endpoint)
bInterfaceClass          : 0x03 (HID - Human Interface Device)
bInterfaceSubClass       : 0x00 (None)
bInterfaceProtocol       : 0x01 (Keyboard)
iInterface               : 0x00 (No String Descriptor)

        ------------------- HID Descriptor --------------------
bLength                  : 0x09 (9 bytes)
bDescriptorType          : 0x21 (HID Descriptor)
bcdHID                   : 0x0111 (HID Version 1.11)
bCountryCode             : 0x00 (00 = not localized)
bNumDescriptors          : 0x01
Descriptor 1:
bDescriptorType          : 0x22 (Class=Report)
wDescriptorLength        : 0x009F (159 bytes)
Error reading descriptor : ERROR_GEN_FAILURE (due to a obscure limitation of the Win32 USB API, see F1 Help)

        ----------------- Endpoint Descriptor -----------------
bLength                  : 0x07 (7 bytes)
bDescriptorType          : 0x05 (Endpoint Descriptor)
bEndpointAddress         : 0x82 (Direction=IN EndpointID=2)
bmAttributes             : 0x03 (TransferType=Interrupt)
wMaxPacketSize           : 0x0010 (16 bytes)
bInterval                : 0x01 (1 ms)

        ---------------- Interface Descriptor -----------------
bLength                  : 0x09 (9 bytes)
bDescriptorType          : 0x04 (Interface Descriptor)
bInterfaceNumber         : 0x02 (Interface 2)
bAlternateSetting        : 0x00
bNumEndpoints            : 0x01 (1 Endpoint)
bInterfaceClass          : 0x03 (HID - Human Interface Device)
bInterfaceSubClass       : 0x00 (None)
bInterfaceProtocol       : 0x02 (Mouse)
iInterface               : 0x00 (No String Descriptor)

        ------------------- HID Descriptor --------------------
bLength                  : 0x09 (9 bytes)
bDescriptorType          : 0x21 (HID Descriptor)
bcdHID                   : 0x0111 (HID Version 1.11)
bCountryCode             : 0x00 (00 = not localized)
bNumDescriptors          : 0x01
Descriptor 1:
bDescriptorType          : 0x22 (Class=Report)
wDescriptorLength        : 0x005E (94 bytes)
Error reading descriptor : ERROR_GEN_FAILURE (due to a obscure limitation of the Win32 USB API, see F1 Help)

        ----------------- Endpoint Descriptor -----------------
bLength                  : 0x07 (7 bytes)
bDescriptorType          : 0x05 (Endpoint Descriptor)
bEndpointAddress         : 0x83 (Direction=IN EndpointID=3)
bmAttributes             : 0x03 (TransferType=Interrupt)
wMaxPacketSize           : 0x0008 (8 bytes)
bInterval                : 0x01 (1 ms)

    ----------------- Device Qualifier Descriptor -----------------
Error                    : request skipped because low-speed device

      -------------------- String Descriptors -------------------
             ------ String Descriptor 0 ------
bLength                  : 0x04 (4 bytes)
bDescriptorType          : 0x03 (String Descriptor)
Language ID[0]           : 0x0409 (English - United States)
             ------ String Descriptor 1 ------
bLength                  : 0x0C (12 bytes)
bDescriptorType          : 0x03 (String Descriptor)
Language 0x0409          : "Razer"
             ------ String Descriptor 2 ------
bLength                  : 0x18 (24 bytes)
bDescriptorType          : 0x03 (String Descriptor)
Language 0x0409          : "Razer Blade"


        +++++++++++++++++ Device Information ++++++++++++++++++
Device Description       : HID Keyboard Device
Device Path 1            : \\?\HID#VID_1532&PID_029F&MI_00#7&1b6c2e0c&0&0000#{884b96c3-56ef-11d1-bc8c-00a0c91405dd} (GUID_DEVINTERFACE_KEYBOARD)
Device Path 2            : \\?\HID#VID_1532&PID_029F&MI_00#7&1b6c2e0c&0&0000#{4d1e55b2-f16f-11cf-88cb-001111000030}\kbd (GUID_DEVINTERFACE_HID)
Kernel Name              : \Device\000000e6
Device ID                : HID\VID_1532&PID_029F&MI_00\7&1B6C2E0C&0&0000
Hardware IDs             : HID\VID_1532&PID_029F&REV_0200&MI_00 HID\VID_1532&PID_029F&MI_00 HID\VID_1532&UP:0001_U:0006 HID_DEVICE_SYSTEM_KEYBOARD HID_DEVICE_UP:0001_U:0006 HID_DEVICE
Driver KeyName           : {4d36e96b-e325-11ce-bfc1-08002be10318}\0000 (GUID_DEVCLASS_KEYBOARD)
Driver                   : \SystemRoot\System32\drivers\kbdhid.sys (Version: 10.0.26100.1930  Date: 2025-12-28  Company: Microsoft Corporation)
Driver Inf               : C:\WINDOWS\inf\keyboard.inf
Legacy BusType           : PNPBus
Class                    : Keyboard
Class GUID               : {4d36e96b-e325-11ce-bfc1-08002be10318} (GUID_DEVCLASS_KEYBOARD)
Service                  : kbdhid
Enumerator               : HID
Location Info            : -
Address                  : 1
Manufacturer Info        : (Standard keyboards)
Capabilities             : 0xA0 (SilentInstall, SurpriseRemovalOK)
Status                   : 0x0180000A (DN_DRIVER_LOADED, DN_STARTED, DN_NT_ENUMERATOR, DN_NT_DRIVER)
First Install Date       : 2025-12-28 13:26:59
Last Arrival Date        : 2026-04-09 17:43:27
EnhancedPowerMgmtEnabled : 0
Power State              : D0 (supported: D0, D1, D2, D3, wake from D0, wake from D1, wake from D2)

             ++++++++++++ Keyboad Information +++++++++++++
Keyboard ID.Type         : 0x51 (HID)
Keyboard ID.Subtype      : 0
Keyboard Mode            : 1
Number of Function Keys  : 12
Number of Indicators     : 3
Number of Keys Total     : 264
Input Data Queue Length  : 1
Key Repeat Minimum.UnitId: 3
Key Repeat Minimum.Rate  : 2 per second
Key Repeat Minimum.Delay : 250 ms
Key Repeat Maximum.UnitId: 3
Key Repeat Maximum.Rate  : 30 per second
Key Repeat Maximum.Delay : 1000 ms
Key Repeat UnitId        : 0
Key Repeat Rate          : 30 per second
Key Repeat Delay         : 250 ms

             ++++++++++++++ HID Information +++++++++++++++
Manufacturer             : Razer
Product                  : Razer Blade
UsagePage                : 0x01 (Generic Desktop Controls)
Usage                    : 0x06 (Keyboard)


        +++++++++++++++++ Device Information ++++++++++++++++++
Device Description       : Razer Blade 16
Device Path 1            : \\?\HID#VID_1532&PID_029F&MI_01&Col01#7&2e9c4a4a&0&0000#{884b96c3-56ef-11d1-bc8c-00a0c91405dd} (GUID_DEVINTERFACE_KEYBOARD)
Device Path 2            : \\?\HID#VID_1532&PID_029F&MI_01&Col01#7&2e9c4a4a&0&0000#{4d1e55b2-f16f-11cf-88cb-001111000030}\kbd (GUID_DEVINTERFACE_HID)
Kernel Name              : \Device\000000e7
Device ID                : HID\VID_1532&PID_029F&MI_01&COL01\7&2E9C4A4A&0&0000
Hardware IDs             : HID\VID_1532&PID_029F&REV_0200&MI_01&Col01 HID\VID_1532&PID_029F&MI_01&Col01 HID\VID_1532&UP:0001_U:0006 HID_DEVICE_SYSTEM_KEYBOARD HID_DEVICE_UP:0001_U:0006 HID_DEVICE
Driver KeyName           : {4d36e96b-e325-11ce-bfc1-08002be10318}\0001 (GUID_DEVCLASS_KEYBOARD)
Driver                   : \SystemRoot\System32\drivers\kbdhid.sys (Version: 10.0.26100.1930  Date: 2025-12-28  Company: Microsoft Corporation)
Driver Inf               : C:\WINDOWS\inf\oem25.inf
Legacy BusType           : PNPBus
Class                    : Keyboard
Class GUID               : {4d36e96b-e325-11ce-bfc1-08002be10318} (GUID_DEVCLASS_KEYBOARD)
Service                  : kbdhid
Enumerator               : HID
Location Info            : -
Address                  : 1
Manufacturer Info        : Razer Inc
Capabilities             : 0xA0 (SilentInstall, SurpriseRemovalOK)
Status                   : 0x0180000A (DN_DRIVER_LOADED, DN_STARTED, DN_NT_ENUMERATOR, DN_NT_DRIVER)
Upper Filters            : RzDev_029f
First Install Date       : 2025-12-28 13:26:59
Last Arrival Date        : 2026-04-09 17:43:27
EnhancedPowerMgmtEnabled : 0
Power State              : D0 (supported: D0, D1, D2, D3, wake from D0, wake from D1, wake from D2)

             ++++++++++++ Keyboad Information +++++++++++++
Keyboard ID.Type         : 0x51 (HID)
Keyboard ID.Subtype      : 0
Keyboard Mode            : 1
Number of Function Keys  : 12
Number of Indicators     : 3
Number of Keys Total     : 264
Input Data Queue Length  : 1
Key Repeat Minimum.UnitId: 4
Key Repeat Minimum.Rate  : 2 per second
Key Repeat Minimum.Delay : 250 ms
Key Repeat Maximum.UnitId: 4
Key Repeat Maximum.Rate  : 30 per second
Key Repeat Maximum.Delay : 1000 ms
Key Repeat UnitId        : 0
Key Repeat Rate          : 30 per second
Key Repeat Delay         : 250 ms

             ++++++++++++++ HID Information +++++++++++++++
Manufacturer             : Razer
Product                  : Razer Blade
UsagePage                : 0x01 (Generic Desktop Controls)
Usage                    : 0x06 (Keyboard)
