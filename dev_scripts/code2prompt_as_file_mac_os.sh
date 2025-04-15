# Install using:
# 1. cargo install code2prompt
# 2. brew install code2prompt
code2prompt . --exclude "*.lock" --exclude ".sqlx/*" --exclude "target" --output-file "$TMPDIR/code.txt" && osascript -e 'set the clipboard to POSIX file "'"$TMPDIR"'/code.txt"'