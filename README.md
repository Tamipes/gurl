## Setup
If you want to set up `gurl init` to use `gurl auth` use this in your .bashrc:
```bash
if command -v gurl &> /dev/null
then
  eval "$(gurl init)"
fi
```
