#!/usr/bin/env python3
"""Print current conditions and a three-day forecast from wttr.in."""
import json
import sys
import urllib.error
import urllib.parse
import urllib.request


def main():
    place = sys.argv[1] if len(sys.argv) > 1 else ""
    us = len(sys.argv) > 2 and sys.argv[2].lower() == "us"
    url = f"https://wttr.in/{urllib.parse.quote(place)}?format=j1"
    try:
        req = urllib.request.Request(url, headers={"User-Agent": "curl/8"})
        with urllib.request.urlopen(req, timeout=20) as resp:
            data = json.load(resp)
    except urllib.error.HTTPError as e:
        sys.exit(f"error: wttr.in answered HTTP {e.code}. The place may not be known: {place!r}")
    except urllib.error.URLError as e:
        sys.exit(f"error: could not reach wttr.in ({e.reason}). Tool network access may be off.")
    except Exception as e:
        sys.exit(f"error: {e}")

    temp, speed = ("F", "mph") if us else ("C", "km/h")
    now = data["current_condition"][0]
    area = data["nearest_area"][0]
    name = ", ".join(x for x in (area["areaName"][0]["value"], area["country"][0]["value"]) if x)
    wind = now["windspeedMiles"] if us else now["windspeedKmph"]
    print(f"{name}: {now['weatherDesc'][0]['value']}, {now['temp_' + temp]}°{temp} "
          f"(feels like {now['FeelsLike' + temp]}°{temp}), wind {wind} {speed}, humidity {now['humidity']}%")
    for day in data["weather"][:3]:
        noon = day["hourly"][len(day["hourly"]) // 2]
        print(f"{day['date']}: {noon['weatherDesc'][0]['value']}, "
              f"{day['mintemp' + temp]} to {day['maxtemp' + temp]}°{temp}, rain chance {noon['chanceofrain']}%")


if __name__ == "__main__":
    main()
