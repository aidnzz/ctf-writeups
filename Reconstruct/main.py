import pyshark

# Load the pcap
cap = pyshark.FileCapture('challenge.pcap', include_raw=True, use_json=True)

# Extract audio data
with open("out.raw", "wb") as f:
    for packet in cap:
        usb = packet.usb
        if usb.transfer_type == "0x00" and usb.src == "host":
            header_len = int(usb.usbpcap_header_len)
            data = packet.get_raw_packet()[header_len:]
            print(len(data))
            f.write(data)




