#!/usr/bin/env bash
set -e

echo "🔍 Searching for [patch] sections in Cargo.toml files..."

# Find all Cargo.toml files and look for '[patch' sections
matches=$(grep -rni --include="Cargo.toml" '\[patch' . || true)

if [ -z "$matches" ]; then
    echo "✅ No [patch] sections found. Your Cargo.toml files are clean."
    exit 0
fi

echo "⚠️ Found the following patch sections:"
echo "$matches"

# Process each found file+line interactively
echo
while IFS= read -r line; do
    file=$(echo "$line" | cut -d: -f1)
    lineno=$(echo "$line" | cut -d: -f2)
    context=$(sed -n "$lineno,$((lineno+5))p" "$file")

    echo
    echo "----------------------"
    echo "📄 File: $file (around line $lineno)"
    echo "$context"
    echo "----------------------"

    read -rp "Do you want to comment out this patch? (y/n): " ans
    if [[ "$ans" == "y" ]]; then
        cp "$file" "$file.bak"
        sed -i "s/^\(\[patch.*\)/# \1/" "$file"
        echo "✅ Commented out the patch and saved backup as $file.bak"
    else
        echo "⏭️ Skipped."
    fi
done <<< "$matches"

echo "🚀 Done checking all patches."
echo "You can now run: cargo build --locked --offline to verify checksums."
