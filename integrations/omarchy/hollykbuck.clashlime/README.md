# clashlime Omarchy bar widget

This Quickshell plugin adds an clashlime icon to the Omarchy bar. Its compact,
two-column panel exposes Mihomo's Rule, Global, and Direct modes plus every
selector proxy group. Groups retain their `runtime.yaml` declaration order;
the selected group's proxies retain Mihomo's order. The panel also shows the
current proxy and can test latency for every proxy in the active group.

The `clashlime` binary must be available on `PATH`. Install the plugin from the
repository root with:

```bash
mkdir -p ~/.config/omarchy/plugins
cp -r integrations/omarchy/hollykbuck.clashlime ~/.config/omarchy/plugins/
omarchy plugin enable hollykbuck.clashlime --section right
```
