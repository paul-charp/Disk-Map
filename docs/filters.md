# Filter grammar

The filter field takes conditions separated by `;`. Empty conditions are ignored. Matching is done on files; a folder shows the sum of its matching files. A bad condition reports `condition: reason` and the previous filter stays active.

```
filter     = condition { ";" condition }
condition  = [ "|" ] body
body       = mask | folder | comparison | tags | attrs | class
```

`|` at the start negates the condition. Keywords and values are case-insensitive.

## How conditions combine

- Name masks, tag conditions and class conditions are selectors: a file must match at least one positive selector (when there is any).
- A negated selector (`|mask`, `|:class:...`, `|:tag:...`) excludes files that match it.
- Size, age and attribute conditions must all hold.
- Folder masks: a file must be inside a folder matching every positive folder mask, and inside none matching a negated one.

## Name masks

`*` matches zero or more characters, `?` one character. The mask is matched against the file name, case-insensitively.

| Filter | Matches |
| --- | --- |
| `*.jpg` | Names ending in `.jpg` |
| `*.jpg;*.png` | Either mask |
| `report?.txt` | `report1.txt`, `reportA.txt` |
| `\|*.tmp` | Everything except `.tmp` files |

## Folder masks

`\mask` matches folders by their own name (not the whole path), with the same wildcards. Folders that match a positive mask are drawn bold. For the scan root the last path component is used.

| Filter | Matches |
| --- | --- |
| `\node_modules` | Files below a folder named `node_modules` |
| `\win*;\data` | Files below a folder `win*` and below a folder `data` |
| `\src;\|\target` | Files below `src` and not below `target` |

At most 64 folder masks are allowed.

## Size

`[keyword]<|>number unit`. The number may have decimals. Units are binary: `b`, `kb`, `mb`, `gb`, `tb`. The unit is required.

| Keyword | Compares |
| --- | --- |
| none, `disksize`, `clustersize` | Allocated size |
| `size`, `filesize`, `logicalsize` | Logical size |

A keyword can be abbreviated: its first letter followed by letters of the full keyword in order (`dsk`, `fsz`, `size`). `>` is greater than, `<` less than.

| Filter | Matches |
| --- | --- |
| `>100mb` | Allocated size above 100 MB |
| `size<11kb` | Logical size below 11 KB |
| `dsk>1gb` | Allocated size above 1 GB |
| `\|size>1mb` | Not larger than 1 MB |

## Age

`[keyword]<|>number unit`. Age is now minus the time stamp, so `>` means older.

| Keyword | Time stamp |
| --- | --- |
| none, `modify`, `modification` | Last write |
| `creation` | Creation |
| `access` | Last access |

Keywords can be abbreviated like size keywords (`a`, `cr`, `mod`). Units: `s`/`sec`/`secs`/`seconds`, `m`/`min`/`mins`/`minutes`, `h`/`hour`/`hours`, `d`/`day`/`days`, `w`/`week`/`weeks`, `month`/`months` (30 days), `y`/`year`/`years` (365 days).

| Filter | Matches |
| --- | --- |
| `modify<7d` | Modified less than 7 days ago |
| `a>1year` | Not accessed for more than a year |
| `creation>2y` | Created more than 2 years ago |

## Tags

`:tag:` or `:tags:` followed by colours. A node's tags are inherited by everything below it. Colours: `1`/`r`/`red`, `2`/`y`/`yellow`, `3`/`g`/`green`, `4`/`b`/`blu`/`blue`, `a`/`all` (any of the four).

Separators: `+` and `,` include, `-` excludes. A file matches when it has at least one included colour (or none are listed) and none of the excluded ones. Short forms `:1`, `:r`, `:red`, `:all` and so on also work.

| Filter | Matches |
| --- | --- |
| `:tag:red` | Tagged red |
| `:tag:red+green-b` | Red or green, not blue |
| `:tag:-b` | Not blue |
| `\|:a` | Not tagged |

## Attributes

`:attr:` or `:attrs:` followed by names with the same separators. A file must have every included attribute and none of the excluded ones.

| Attribute | Names |
| --- | --- |
| archive | `archive`, `archi`, `arch`, `arc`, `ar`, `a` |
| system | `system`, `sys`, `s` |
| read-only | `readonly`, `rdonly`, `ronly`, `rdo`, `ro`, `r` |
| hidden | `hidden`, `hidn`, `hid`, `h` |
| compressed | `compressed`, `comprsd`, `compr`, `cpr`, `c` |
| encrypted | `encrypted`, `encrptd`, `encptd`, `enc`, `e`, `crypted`, `crypt`, `cptd` |
| offline | `offline`, `off`, `ol`, `o` |
| temporary | `temporary`, `temp`, `tmp`, `t` |
| not indexed | `notindexed`, `notindx`, `notidx`, `noidx`, `nidx`, `ni` |
| sparse | `sparse`, `sp` |

| Filter | Matches |
| --- | --- |
| `:attr:hidden+system` | Hidden and system |
| `:attr:a-ro,h` | Archive and hidden, not read-only |
| `\|:attr:temp` | Not temporary |

## Classes

`:class:Name` matches files whose extension belongs to the file class (case-insensitive name). Default classes: Video, Images, Archives, Code, Documents, Audio. Classes and their extensions are editable in Settings, Classes.

| Filter | Matches |
| --- | --- |
| `:class:Video;size>1gb` | Videos larger than 1 GB |
| `\|:class:Audio` | Everything except audio |

A class condition is a selector, so `:class:Video;:class:Audio` matches either class.
