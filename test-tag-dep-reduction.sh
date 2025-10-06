#!/bin/bash
set -e

# Test script for tag dependency reduction bug fix
# This verifies that changes created AFTER a tag depend on the tag, not individual changes

echo "=== Tag Dependency Reduction Test ==="
echo ""

# Create a temporary test repository
TEST_DIR=$(mktemp -d)
echo "Creating test repository in $TEST_DIR"
cd "$TEST_DIR"

# Initialize repository
atomic init test-repo
cd test-repo

echo ""
echo "=== Step 1: Create initial changes ==="
echo "Creating change A..."
echo "Content A" > a.txt
atomic add a.txt
CHANGE_A=$(atomic record -m "Change A" --author "Test <test@example.com>" 2>&1 | grep "Hash:" | awk '{print $2}')
echo "Change A: $CHANGE_A"

echo "Creating change B..."
echo "Content B" > b.txt
atomic add b.txt
CHANGE_B=$(atomic record -m "Change B" --author "Test <test@example.com>" 2>&1 | grep "Hash:" | awk '{print $2}')
echo "Change B: $CHANGE_B"

echo "Creating change C..."
echo "Content C" > c.txt
atomic add c.txt
CHANGE_C=$(atomic record -m "Change C" --author "Test <test@example.com>" 2>&1 | grep "Hash:" | awk '{print $2}')
echo "Change C: $CHANGE_C"

echo ""
echo "=== Step 2: Create tag ==="
echo "Creating tag 'test-tag'..."
TAG_OUTPUT=$(atomic tag create --version "1.0.0" 2>&1)
echo "$TAG_OUTPUT"
TAG_HASH=$(echo "$TAG_OUTPUT" | grep -o '[A-Z0-9]\{53\}' | head -1)
echo "Tag hash: $TAG_HASH"

echo ""
echo "=== Step 3: Create new change after tag ==="
echo "Creating change D (should depend ONLY on tag)..."
echo "Content D" > d.txt
atomic add d.txt
CHANGE_D=$(atomic record -m "Change D - Finale" --author "Test <test@example.com>" 2>&1 | grep "Hash:" | awk '{print $2}')
echo "Change D: $CHANGE_D"

echo ""
echo "=== Step 4: Verify dependencies ==="
echo "Checking change D's dependencies..."

# Export change D and check its dependencies
atomic change "$CHANGE_D" > /tmp/change_d_info.txt 2>&1

# Check if the dependencies section exists
if grep -q "# Dependencies" /tmp/change_d_info.txt; then
    echo ""
    echo "Dependencies found in change D:"
    grep -A 20 "# Dependencies" /tmp/change_d_info.txt | head -20

    # Count dependencies (look for lines starting with [number])
    DEP_COUNT=$(grep -A 100 "# Dependencies" /tmp/change_d_info.txt | grep -E "^\[[0-9]+\]" | wc -l | tr -d ' ')
    echo ""
    echo "Total dependencies: $DEP_COUNT"

    # Check if tag is in dependencies
    if grep -A 100 "# Dependencies" /tmp/change_d_info.txt | grep -q "$TAG_HASH"; then
        echo "✅ SUCCESS: Tag $TAG_HASH is in dependencies"
    else
        echo "❌ FAILURE: Tag $TAG_HASH is NOT in dependencies"
    fi

    # Check if old changes are NOT in dependencies (they should be consolidated)
    FOUND_OLD_CHANGES=0
    if grep -A 100 "# Dependencies" /tmp/change_d_info.txt | grep -q "$CHANGE_A"; then
        echo "❌ FAILURE: Change A is in dependencies (should be consolidated by tag)"
        FOUND_OLD_CHANGES=1
    fi
    if grep -A 100 "# Dependencies" /tmp/change_d_info.txt | grep -q "$CHANGE_B"; then
        echo "❌ FAILURE: Change B is in dependencies (should be consolidated by tag)"
        FOUND_OLD_CHANGES=1
    fi
    if grep -A 100 "# Dependencies" /tmp/change_d_info.txt | grep -q "$CHANGE_C"; then
        echo "❌ FAILURE: Change C is in dependencies (should be consolidated by tag)"
        FOUND_OLD_CHANGES=1
    fi

    if [ $FOUND_OLD_CHANGES -eq 0 ]; then
        echo "✅ SUCCESS: Old changes are NOT in dependencies (consolidated by tag)"
    fi

    # Check if dependency count is 1 (just the tag)
    if [ "$DEP_COUNT" -eq 1 ]; then
        echo "✅ SUCCESS: Exactly 1 dependency (O(1) dependency reduction working!)"
    else
        echo "⚠️  WARNING: Expected 1 dependency, found $DEP_COUNT"
        echo ""
        echo "Full change D info for debugging:"
        cat /tmp/change_d_info.txt
    fi
else
    echo "❌ Could not find dependencies section in change D"
    echo ""
    echo "Full output:"
    cat /tmp/change_d_info.txt
fi

echo ""
echo "=== Step 5: Show full log ==="
atomic log --limit 10

echo ""
echo "=== Cleanup ==="
cd /
rm -rf "$TEST_DIR"
echo "Removed test repository"

echo ""
echo "=== Test Complete ==="
