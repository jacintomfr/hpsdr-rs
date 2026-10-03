[← RX-888 Mk2](20-rx888-mk2.md) | [Index](README.md)

# Connecting a radio directly to the computer (no router)

A Hermes-Lite 2 (or any openHPSDR network radio) can be plugged straight into
a PC or Raspberry Pi with an Ethernet cable, or through a USB-to-Ethernet
adapter, with no router or DHCP server in between. This works, but the
computer's network interface **must have an IP address in the same subnet as
the radio** or the **Discover HPSDR Radios** window will stay empty: discovery
is a UDP broadcast, and it can only go out over an interface that has an
address.

A radio with no DHCP server to talk to falls back to a *link-local* address in
`169.254.0.0/16` (the Windows title bar of a connected radio shows it, e.g.
`HermesLite2 (P1 v7.4) at 169.254.19.221`). The computer's interface needs a
`169.254.x.x` address too.

## Windows

Windows does this on its own ("APIPA" automatic private addressing): when the
adapter is set to *Obtain an IP address automatically* and finds no DHCP
server, it gives itself a `169.254.x.x` address after a short wait (up to about
a minute after plugging the cable in).

* Check with `ipconfig`: the adapter should list an *Autoconfiguration IPv4
  Address* of `169.254.x.x`.
* If the adapter has an old fixed IP from another network, switch it back to
  *Obtain an IP address automatically* (Adapter properties -> Internet
  Protocol Version 4), or set a fixed `169.254.x.y` address with netmask
  `255.255.0.0` yourself.
* If the radio is still not listed, check that Windows Firewall allows
  hpsdr-rs on the network profile of that adapter (a directly connected
  adapter often shows up as an "Unidentified network", which Windows treats as
  Public).

This is how a Hermes-Lite 2 connected through a USB 3.0 gigabit adapter was
used on Windows.

## Linux / Raspberry Pi OS (NetworkManager)

Unlike Windows, NetworkManager's default *Automatic (DHCP)* method does **not**
fall back to a link-local address: it tries DHCP, gives up (`dhcp4 (eth1): state
changed no lease`, then `Activation: failed`) and leaves the interface with
**no IPv4 address at all**, even though the link is up. The symptom is a
healthy cable (`ip link` shows `UP`, 1000 Mb/s), an interface with no `inet`
address in `ip -br addr`, and an empty discovery window.

Switch that connection to link-local addressing (replace the connection name
and keep it tied to the radio's interface only -- your normal LAN interface is
not touched):

```bash
nmcli con show                                   # find the connection for the adapter
sudo nmcli con modify "Wired connection 2" ipv4.method link-local ipv6.method ignore
sudo nmcli con up "Wired connection 2"
ip -br addr show eth1                            # should now show 169.254.x.x/16
```

Seen on a Raspberry Pi 5 (Debian 13 "trixie") with a Realtek RTL8153 USB 3.0
gigabit adapter (`eth1`) next to the built-in `eth0` on the normal LAN: after
the change `eth1` got a `169.254.x.x` address.

### Without NetworkManager

Give the interface a fixed address in the radio's subnet, for example for a
radio at `169.254.19.221` on `eth1`:

```bash
sudo ip addr add 169.254.19.10/16 dev eth1
sudo ip link set eth1 up
```

(`ip addr add` does not survive a reboot; use your distribution's own network
configuration to make it permanent.)

### Checking that the radio is reachable

```bash
ip -br addr show eth1            # has a 169.254.x.x address?
ip -s link show eth1             # RX packet counter should grow
ping -c 3 169.254.19.221         # the radio's address, from its title bar / log
```

If you run a firewall (`ufw`, `firewalld`), allow UDP from the radio's subnet:
discovery uses UDP port 1024 and the data streams use further UDP ports.

Two interfaces on different subnets (a normal LAN plus the radio's
`169.254.0.0/16` link) is fine -- the radio's subnet is reached only through its
own interface.
