---
status: accepted
---

# Agents use the web through their own Web tab in the chief's window

An agent's web tools, `web_open`, `web_read`, `web_console`,
`web_wait`, `web_click` and `web_type`, act on a page in a Web tab of
the agent's own, in the chief's window. The server sends each call to
the window that draws Web panels and that the chief used last, as a
`web` ask of the client protocol, and gives the agent the window's
answer. With no such window, the call is refused, and the agent asks
the chief to open one. The tab is named after the agent, opens behind
what the chief is looking at, keeps a session of its own in memory
apart from the chief's, and closes when the agent ends. A page on
another machine needs the network, as a shell command does: `web_open`
of such an address passes the gate as a networked call, and an agent's
page that reaches one any other way without the network is blanked.

The page's own document carries out the acts that read or change it,
through a script the window runs in agents' pages before their own.
`web_read` numbers the page's links, buttons and fields for
`web_click` and `web_type`; `web_console` gives what Chromium reports
of the page's console, uncaught errors included, and its failed loads.

## Considered options

- A headless browser on the server. It would work with no window and
  for a remote server, but the chief would not see the agent's page,
  and the server would carry a browser of its own. Left out.
- Opening the window when an agent needs one. It puts a window on the
  chief's screen unasked, and cannot for a server on another machine.
- `web_screenshot`. Deferred until the tools' results can carry images
  to the models: a picture of the page is what a layout is checked by.
- Raw HTML and CSS as the page. The agent wrote those; what the browser
  built from them, after the page's scripts ran, is what it cannot see
  otherwise. `web_read` gives the HTML when asked.

## Consequences

- The agents' web tools work only while a window with Web panels is
  open.
- A click is a script's click: a page that only trusts input it can
  tell came from a person does not take it.
- Pages fetch from other machines as they load, fonts and scripts
  alike: the network rule holds for the page itself, not for what it
  loads.
