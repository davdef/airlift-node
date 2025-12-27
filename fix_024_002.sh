#!/bin/bash
# fix_024_002.sh

# 1. Die problematische Zeile in node.rs fixen
echo "Fixing info! macro line in node.rs..."
sed -i '50s/info!("Flow "{}": Added consumer "{}"", self.name, consumer_name);/info!("Flow \"{}\": Added consumer \"{}\"", self.name, consumer_name);/' src/core/node.rs

# 2. Nochmal überprüfen
echo "=== Korrigierte Zeile 50 in node.rs ==="
sed -n '49,53p' src/core/node.rs

# 3. Test kompilieren
echo -e "\nTesting compilation..."
cargo check
