# Downloads and technical references

<a href="https://github.com/embedconsult/quadesc/tree/main/documentation/source/quadesc">Editable manual source</a> · <a href="print.html">Printable manual</a>

The repository contains the authored Markdown and shared artwork. The printable view brings the chapters together in one document.

## Circuit and interface data

- {download}`Connector pad numbers, nets and board sides <../../reference/mainboard-connector-pads.csv>`.
- {download}`Board GPIO assignments and connector access <../../reference/quadesc-gpio-map.csv>`.
- {download}`All 30 ADC routes, MCU pads and mainboard nets <../../reference/mainboard-analog-routes.csv>`.

The connector and GPIO downloads help you follow a signal from its board net to an accessible pad. The analog-route download follows each ADC selection through the MCU and module connector to its mainboard signal.

## Manufacturer references

- [TI AM13E230x datasheet](https://www.ti.com/lit/ds/sprspc3/sprspc3.pdf).
- [TI AM13E230x Technical Reference Manual](https://www.ti.com/lit/ug/sprujf2/sprujf2.pdf).

ADC acquisition and sequence details are in TRM chapter 21; PWM time-base and action-qualifier details are in chapter 26.
