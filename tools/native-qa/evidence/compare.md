# Design vs native: measured

## animate

Images: `animate-side.png`, `animate-diff.png`. Pixels differing by more than 24 in a channel: 140568 of 1296000 (10.8%).

| Region | Design DOM (x, y, w, h · colour · radius) | Design pixels | Native pixels | Verdict |
|---|---|---|---|---|
| tools | 16, 84, 64, 509 · #16161A · 24 | 16, 84, 64, 509 · #16161A · r~23.9 | 16, 84, 64, 509 · #16161A · r~23.9 | ok |
| canvas | 92, 84, 1008, 509 · #FFFFFF · 24 | 92, 84, 1008, 509 · #FFFFFF · r~23.9 | 92, 84, 1000, 509 · #FFFFFF · r~23.9 | FAIL: w 1000 vs 1007.5 (-8) |
| panel | 1112, 84, 312, 509 · #FFFFFF · 24 | 1112, 84, 312, 509 · #FFFFFF · r~23.9 | 1104, 84, 320, 509 · #FFFFFF · r~23.9 | FAIL: x 1104 vs 1111.5 (-8); w 320 vs 312.5 (+8) |
| strip | 16, 605, 1408, 279 · #16161A · 22 | 16, 605, 1408, 279 · #16161A · r~20.5 | 16, 605, 1408, 279 · #16161A · r~20.5 | ok |
| header (content above the body) | box 16, 16, 1408, 56 | 16, 20, 1408, 48 | 16, 20, 1408, 48 | ok |

Key colours the design region uses (at least 2% of it) that the native region lacks (±6):

- **header**: design #F6F4EF 59% #EFECE4 15% #D9D2FF 6% #E8492F 6% #16161A 4% #FFFFFF 4%; native #F6F4EF 60% #EFECE4 14% #D9D2FF 6% #E8492F 6% #16161A 4% #FFFFFF 4%; missing: none
- **tools**: design #16161A 57% #2A2A30 30% #E8492F 5%; native #16161A 57% #2A2A30 31% #E8492F 5% #F6F4EF 2% #FFD84D 1%; missing: none
- **canvas**: design #FFFFFF 78% #1E1E24 9% #F6F4EF 3%; native #FFFFFF 99% #F6F4EF 1%; missing: #1E1E24
- **panel**: design #FFFFFF 67% #EFECE4 5%; native #FFFFFF 61% #EFECE4 5% #F6F4EF 1% #FFD84D 1% #16161A 1% #8C7B9E 1%; missing: none
- **strip**: design #1E1E24 35% #16161A 27% #34343C 16% #2A2A30 3% #4A3F7A 2%; native #1E1E24 34% #16161A 31% #34343C 19% #2A2A30 5% #4A3F7A 2% #2B2B32 2%; missing: none

Design elements: 4 workspace tabs (Draw, Animate, Colour, Review); 8 tool-column buttons (Brush (B), Eraser (E), Fill (G), Lasso fill (F), Select (M), Transform (V), Eyedropper (I), Colour: Line black (open colours)); panel tabs: Colour, Brushes, Layer; header buttons: File, View, Draw, Animate, Colour, Review, Undo, Redo, Draw together, Export.

## draw

Images: `draw-side.png`, `draw-diff.png`. Pixels differing by more than 24 in a channel: 48344 of 1296000 (3.7%).

| Region | Design DOM (x, y, w, h · colour · radius) | Design pixels | Native pixels | Verdict |
|---|---|---|---|---|
| tools | 16, 84, 64, 736 · #16161A · 24 | 16, 84, 64, 736 · #16161A · r~23.9 | 16, 84, 64, 736 · #16161A · r~23.9 | ok |
| canvas | 92, 84, 1008, 736 · #FFFFFF · 24 | 92, 84, 1008, 736 · #FFFFFF · r~23.9 | 92, 84, 1000, 736 · #FFFFFF · r~23.9 | FAIL: w 1000 vs 1007.5 (-8) |
| panel | 1112, 84, 312, 736 · #FFFFFF · 24 | 1112, 84, 312, 736 · #FFFFFF · r~23.9 | 1104, 84, 320, 736 · #FFFFFF · r~23.9 | FAIL: x 1104 vs 1111.5 (-8); w 320 vs 312.5 (+8) |
| strip | 16, 832, 1408, 52 · #16161A · 22 | 16, 832, 1408, 52 · #16161A · r~20.5 | 16, 832, 1408, 52 · #16161A · r~20.5 | ok |
| header (content above the body) | box 16, 16, 1408, 56 | 16, 20, 1408, 48 | 16, 20, 1408, 48 | ok |

Key colours the design region uses (at least 2% of it) that the native region lacks (±6):

- **header**: design #F6F4EF 59% #EFECE4 16% #D9D2FF 6% #E8492F 6% #FFFFFF 4% #16161A 4%; native #F6F4EF 60% #EFECE4 15% #D9D2FF 6% #E8492F 6% #FFFFFF 4% #16161A 4%; missing: none
- **tools**: design #16161A 66% #2A2A30 24% #E8492F 3%; native #16161A 66% #2A2A30 25% #E8492F 4% #F6F4EF 1% #FFD84D 0%; missing: none
- **canvas**: design #FFFFFF 95%; native #FFFFFF 99% #F6F4EF 1%; missing: none
- **panel**: design #FFFFFF 84% #F6F4EF 6% #EFECE4 4%; native #FFFFFF 84% #F6F4EF 6% #EFECE4 4% #E8492F 1% #E4E1D8 1%; missing: none
- **strip**: design #16161A 73% #F6F4EF 12% #2A2A30 7% #E8492F 4%; native #16161A 68% #F6F4EF 16% #2A2A30 7% #E8492F 4% #FFD84D 2%; missing: none

Design elements: 4 workspace tabs (Draw, Animate, Colour, Review); 9 tool-column buttons (Brush (B), Eraser (E), Fill (G), Lasso fill (F), Select (M), Transform (V), Eyedropper (I), Text (T), Colour: Line black (open colours)); panel tabs: Brushes, Colour, Layer; header buttons: File, View, Draw, Animate, Colour, Review, Undo, Redo, Draw together, Export.

## colour

Images: `colour-side.png`, `colour-diff.png`. Pixels differing by more than 24 in a channel: 135878 of 1296000 (10.5%).

| Region | Design DOM (x, y, w, h · colour · radius) | Design pixels | Native pixels | Verdict |
|---|---|---|---|---|
| tools | 16, 84, 64, 651 · #16161A · 24 | 16, 84, 64, 651 · #16161A · r~23.9 | 16, 84, 64, 646 · #16161A · r~23.9 | FAIL: h 646 vs 651 (-5) |
| canvas | 92, 84, 1008, 651 · #FFFFFF · 24 | 92, 84, 1008, 651 · #FFFFFF · r~23.9 | 92, 84, 1000, 646 · #FFFFFF · r~23.9 | FAIL: w 1000 vs 1007.5 (-8); h 646 vs 651 (-5) |
| panel | 1112, 84, 312, 651 · #FFFFFF · 24 | 1112, 84, 312, 651 · #FFFFFF · r~23.9 | 1104, 84, 320, 646 · #FFFFFF · r~23.9 | FAIL: x 1104 vs 1111.5 (-8); w 320 vs 312.5 (+8); h 646 vs 651 (-5) |
| strip | 16, 747, 1408, 137 · #16161A · 22 | 16, 747, 1408, 137 · #16161A · r~20.5 | 16, 742, 1408, 142 · #16161A · r~20.5 | FAIL: y 742 vs 747 (-5); h 142 vs 137 (+5) |
| header (content above the body) | box 16, 16, 1408, 56 | 16, 20, 1408, 48 | 16, 20, 1408, 48 | ok |

Key colours the design region uses (at least 2% of it) that the native region lacks (±6):

- **header**: design #F6F4EF 59% #EFECE4 15% #D9D2FF 6% #E8492F 6% #FFFFFF 4% #16161A 4%; native #F6F4EF 60% #EFECE4 15% #D9D2FF 6% #E8492F 6% #FFFFFF 4% #16161A 4%; missing: none
- **tools**: design #16161A 73% #2A2A30 16% #E8492F 4% #F2C9A8 2%; native #16161A 75% #2A2A30 16% #E8492F 4% #F6F4EF 2% #FFD84D 1%; missing: #F2C9A8
- **canvas**: design #FFFFFF 87% #E8492F 4% #F2C9A8 4%; native #FFFFFF 98% #F6F4EF 2%; missing: #E8492F, #F2C9A8
- **panel**: design #FFFFFF 87% #EFECE4 4% #F6F4EF 3%; native #FFFFFF 85% #EFECE4 4% #F6F4EF 4% #16161A 1% #E8492F 1%; missing: none
- **strip**: design #16161A 64% #FFFFFF 14% #24242A 14%; native #16161A 54% #FFFFFF 19% #24242A 19% #E8492F 2% #F2C9A8 1%; missing: none

Design elements: 4 workspace tabs (Draw, Animate, Colour, Review); 6 tool-column buttons (Fill (G), Lasso fill (F), Eyedropper (I), Brush (B), Eraser (E), Colour: Skin (open colours)); panel tabs: Colour, Brushes, Layer; header buttons: File, View, Draw, Animate, Colour, Review, Undo, Redo, Draw together, Export.

## review

Images: `review-side.png`, `review-diff.png`. Pixels differing by more than 24 in a channel: 105726 of 1296000 (8.2%).

| Region | Design DOM (x, y, w, h · colour · radius) | Design pixels | Native pixels | Verdict |
|---|---|---|---|---|
| tools | none | none | none | ok (none in either) |
| canvas | 16, 84, 1083, 694 · #FFFFFF · 24 | 16, 84, 1083, 694 · #FFFFFF · r~23.9 | 16, 84, 1076, 694 · #FFFFFF · r~23.9 | FAIL: w 1076 vs 1083.5 (-7) |
| panel | 1111, 84, 313, 694 · #FFFFFF · 24 | 1111, 84, 313, 694 · #FFFFFF · r~23.9 | 1104, 84, 320, 694 · #FFFFFF · r~23.9 | FAIL: x 1104 vs 1111.5 (-7); w 320 vs 312.5 (+7) |
| strip | 16, 790, 1408, 94 · #16161A · 22 | 16, 790, 1408, 94 · #16161A · r~20.5 | 16, 790, 1408, 94 · #16161A · r~20.5 | ok |
| header (content above the body) | box 16, 16, 1408, 56 | 16, 20, 1408, 48 | 16, 20, 1408, 48 | ok |

Key colours the design region uses (at least 2% of it) that the native region lacks (±6):

- **header**: design #F6F4EF 59% #EFECE4 15% #D9D2FF 6% #E8492F 6% #FFFFFF 4% #16161A 4%; native #F6F4EF 60% #EFECE4 15% #D9D2FF 6% #E8492F 6% #FFFFFF 4% #16161A 4%; missing: none
- **canvas**: design #FFFFFF 87% #E8492F 4% #F2C9A8 4%; native #FFFFFF 99% #F6F4EF 1%; missing: #E8492F, #F2C9A8
- **panel**: design #FFFFFF 69% #F6F4EF 17% #FFF4D1 8%; native #FFFFFF 80% #FFF4D1 9% #F6F4EF 7% #16161A 1%; missing: none
- **strip**: design #16161A 82% #2A2A30 5% #34343C 4% #E8492F 3% #5FD08A 2%; native #16161A 82% #2A2A30 5% #34343C 4% #E8492F 3% #5FD08A 2% #F6F4EF 0%; missing: none

Design elements: 4 workspace tabs (Draw, Animate, Colour, Review); 0 tool-column buttons (); panel tabs: none; header buttons: File, View, Draw, Animate, Colour, Review, Undo, Redo, Draw together, Export.

