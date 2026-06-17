# Make hook command paths quote-safe for a $HOME containing spaces.
def quote_path:
  if startswith("\"") then .
  else
    (index(" ")) as $i
    | if $i == null then "\"\(.)\""
      else "\"\(.[0:$i])\"\(.[$i:])"
      end
  end;
.hooks |= (
  to_entries
  | map(.value |= map(.hooks |= map(.command |= quote_path)))
  | from_entries
)