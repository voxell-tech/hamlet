---
name: weather
description: Current weather and a three-day forecast for a place, in metric or US units.
usage: python3 skills/weather/scripts/weather.py "<place>" [metric|us]
---

Needs tool network access and python3. Uses wttr.in, which needs no API key.

```sh
python3 skills/weather/scripts/weather.py "<place>" [metric|us]
```

Use the unit system the user prefers (check NOTES.md); metric is the default.
With no place given, wttr.in guesses from the network address, which is often
wrong, so ask the user or use the place in NOTES.md.
