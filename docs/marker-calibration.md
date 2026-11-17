# Kalibracja i ręczny wybór widocznych markerów

Biblioteka `mtc-vision` proponuje kolor i kształt kompaktowych komponentów w wybranym ROI mapy. Użytkownik wskazuje konkretną ikonę i przypisuje jej rolę. Propozycje nie identyfikują drużyny, przeciwnika, własnego pojazdu ani parametrów uzbrojenia. Wynik kliknięcia zawiera tylko geometrię pikseli obrazu; przeliczenie na skalibrowaną mapę należy do integracji.

## API kalibracji

```rust
propose_marker_signatures(image, map_roi, timestamp_ms, now_ms)
    -> Result<MarkerProposalReport, String>
propose_marker_signatures_with_config(image, map_roi, timestamp_ms, now_ms, config)
    -> Result<MarkerProposalReport, String>
calibrate_marker_signature(image, map_roi, click, timestamp_ms, now_ms)
    -> Result<MarkerCalibration, String>
```

`map_roi` i `click` są normalizowane do pełnej klatki, w przedziale 0–1. Przed wywołaniem integracja musi jawnie wybrać widoczny ROI mapy lub potwierdzić dopasowanie lokalnego obrazu referencyjnego. API kalibracji nie jest rozpoznawaniem semantycznym mapy i nie zwraca współrzędnych świata.

`MarkerProposalReport` w JSON:

```text
schemaVersion: 1
status: proposed | blocked | stale
timestampMs, frameWidth, frameHeight
mapRoi, observedMapWidth, observedMapHeight
proposals: [{
  id, signature: MarkerSignature,
  framePositionNormalized, boundingBox,
  pixelCount, aspectRatio, fillRatio, clipped, colorSpread, confidence
}]
warnings: string[]
```

`MarkerCalibration` zawiera te same metadane źródła, `click`, `proposal: MarkerSignatureProposal | null`, `warnings` oraz stan `calibrated | ambiguous | blocked | stale`. `proposal.id` identyfikuje tylko komponent bieżącej klatki. `proposal.signature.id` opisuje klasę koloru i nie jest trwałą tożsamością pojazdu. Każda proponowana sygnatura ma rolę `unassigned`.

Segmentacja korzysta z sąsiedztwa ośmiu pikseli i ograniczonej różnicy RGB. Proponuje wyraźnie chromatyczne komponenty oraz bardzo jasne lub ciemne ikony. Mediana RGB i 95. percentyl odchylenia koloru dają proponowaną tolerancję; powierzchnia, proporcje i wypełnienie dają początkowe progi kształtu. To heurystyka obrazu, więc np. kolorowy tekst lub element terenu może również pojawić się w propozycjach. Użytkownik musi potwierdzić właściwą ikonę. Brak odpowiedniego komponentu wymaga ręcznych progów RGB.

Domyślne limity: 4 miliony pikseli ROI, 1024 rozpatrzone komponenty, 64 propozycje, 3–4096 pikseli komponentu, bok do 96 px i co najmniej 8% wypełnienia. Nadmiar kandydatów blokuje całą listę zamiast zwracać arbitralny podzbiór. Piksele o alfa mniejszym niż 128 nie tworzą propozycji. Kalibracja przez kliknięcie przeszukuje najwyżej 97 × 97 px przy wskazanym punkcie, więc kolorowy szum poza tym obszarem nie ustala sygnatury.

Kliknięcie może trafić w pusty środek otoczonej widoczną obwódką ikony. Kilka komponentów w podobnej odległości, brak komponentu lub przycięta ikona nie zatwierdzają sygnatury. Przezroczystość, bardzo małe ikony, silna zmiana koloru i połączone oznaczenia mogą wymagać ręcznej kalibracji.

## Jednostki i źródło czasu

Proponowane granice powierzchni są w pikselach obserwowanego ROI. Przed użyciem w `MarkerConfig` należy przeliczyć `minComponentPixels` i `maxComponentPixels` na piksele mapy referencyjnej. Dla potwierdzonej pełnej mapy współczynnik powierzchni to `referenceWidth * referenceHeight / (observedMapWidth * observedMapHeight)`. Przy dopasowanym fragmencie mapy należy użyć kwadratu skali transformacji obrazu do referencji; stosunek rozmiarów przyciętego ROI nie określa tej skali. RGB i proporcje kształtu nie są jednostkami świata.

Wszystkie czasy oznaczają rzeczywisty czas źródłowej klatki. Klatka przyszła lub starsza niż 2000 ms jest blokowana; świeżość jest sprawdzana również po segmentacji. Stary podgląd nie może dostać nowego znacznika czasu. Jeżeli użytkownik długo ogląda podgląd, integracja musi pobrać i sprawdzić nową klatkę przed zatwierdzeniem pozycji. Zniknięcie lub przesunięcie ikony wymaga ponownego wskazania.

## Wybór spośród podobnych ikon

```rust
select_confirmed_marker(candidates, hint, previous_state, timestamp_ms, now_ms, config)
    -> Result<MarkerSelectionResult, String>
```

`candidates` to bieżące obserwacje z `detect_markers`, już przeliczone do pikseli mapy referencyjnej. `hint.confirmedPoint` to przeliczony punkt z jawnego kliknięcia. Żadna arbitralna pozycja początkowa nie zastępuje tego potwierdzenia.

`MarkerSelectionHint`:

```text
selectionId, signatureId, role: own | target | reference
confirmedPoint: { x, y }                  # piksele mapy referencyjnej
timestampMs                              # klatka jawnego wskazania
scope, frameWidth, frameHeight, mapWidth, mapHeight
```

`MarkerSelectionConfig`:

```text
scope, frameWidth, frameHeight, mapWidth, mapHeight
maxAgeMs                                 # nie więcej niż 2000
initialRadiusPixels, baseGatePixels, maxAssociationRadiusPixels
maxSpeedPixelsPerSecond                   # jawny limit, nie parametr MTC
ambiguityMarginPixels, minConfidence      # co najmniej 0.6
```

`scope` integracja wiąże ze źródłowym oknem, epoką przechwytywania, mapą, kalibracją oraz konfiguracją kolorów i kształtu. Rozmiary klatki i mapy są sprawdzane niezależnie. Zmiana któregokolwiek elementu wymaga nowego wskazania oraz usunięcia historii ruchu. Nie należy wiązać scope ze zmieniającym się położeniem komponentu ani z przewijaniem dopasowanej mapy, gdy geometria referencyjna pozostaje prawidłowa.

`MarkerSelectionResult`:

```text
status: collecting | ready | ambiguous | stale | blocked | rejected
selected: MarkerObservation | null
trackId: string | null
count, confidence, timestampMs, warning
state: { hint, last: MarkerObservation, previous: MarkerObservation | null, count } | null
```

Pierwsza zgodna obserwacja jest `collecting` i ma surową pewność. Dopiero dwie różne świeże klatki zgodne z bramką położenia i ruchu dają `ready` oraz heurystyczną pewność 0,9. Automatyczne współrzędne wolno stosować dopiero przy `ready`. `selected.role` pochodzi z jawnego `hint.role`. Do `PositionSample.sourceId` należy przekazać `trackId`, a nie wspólne ID koloru.

Przy pierwszej klatce punkt musi być blisko ręcznego wskazania. Później porównanie korzysta z ostatnich rzeczywistych obserwacji oraz przybliżenia stałej prędkości, gdy obie obserwacje są świeże. Bramka ma jawny maksymalny promień i limit prędkości. Kandydaci o podobnej odległości, teleport, luka ponad 2 s lub niespójna geometria nie zwracają wybranego punktu. Predykcja służy wyłącznie porównaniu widocznych ikon; nie jest zwracana jako obserwowana lub ukryta pozycja. Duplikat tej samej klatki ma stan `blocked`, nie zwiększa liczby obserwacji i zachowuje stan do następnej różnej klatki.

Jedna wspólna sygnatura `unassigned` może wykrywać kilka identycznie kolorowych ikon. Dwa jawne hinty, np. własny i cel, wybierają z tej klasy osobne komponenty i mają różne `selectionId`. Nie należy tworzyć dwóch nierozróżnialnych sygnatur kolorystycznych z różnymi rolami: istniejąca walidacja `MarkerConfig` słusznie je odrzuca. Gdy role wskazują tę samą ikonę lub podobne ikony krzyżują się, integracja powinna wymagać ponownego wyboru. Automat nie odtwarza tożsamości po utracie obserwacji.

Integracja zachowuje niezaznaczone komponenty do podglądu, ale nie podaje ich jako własnej pozycji lub celu. Przy wyniku bez `selected` musi wstrzymać wyprzedzenie i wyczyścić próbki odpowiadającego trackera. Błąd lub zmiana źródła nie pozwala korzystać z poprzedniego punktu. Dla wygody można zachować sam wzorzec koloru i kształtu; hint oraz stan śledzenia odnoszą się wyłącznie do sesji.

## Weryfikacja

Regresje syntetyczne sprawdzają kolory bez przypisanych ról, kilka ikon tego samego koloru, normalizowane kliknięcie pełnego obrazu, puste środki obwódek, alfa i niewielkie odchylenie barwy, wyjście poza ROI, obcięcie, remisy, nadmiar komponentów, przyszłe i stare klatki. Polityka wyboru ma osobne testy kolejności kandydatów, dwóch świeżych obserwacji, bramkowania ruchem, skoków, niepoprawnej historii oraz resetów mapy, źródła i geometrii. Te testy nie potwierdzają wyglądu wszystkich ikon w aktualnym MTC.

Polecenia: `cargo test -p mtc-vision`, `cargo clippy -p mtc-vision --all-targets -- -D warnings`, `cargo fmt -p mtc-vision -- --check`.

Obrazy są przetwarzane tylko w pamięci. Biblioteka nie zapisuje klatek, nie korzysta z pamięci gry i nie steruje pojazdem ani strzelaniem.


## Integracja Windows w 0.4

`capture_marker_setup` zachowuje jedną klatkę wyłącznie w RAM i zwraca miniaturę z osobnymi wymiarami podglądu i źródła. `calibrate_visible_marker` używa tokenu oraz źródłowego czasu klatki; nie odświeża czasu kliknięcia. Natywna aprobata wiąże wybrany komponent z oknem, backendem, epoką, mapą i kalibracją. Stop, zmiana mapy/źródła lub nieaktualna obserwacja wymagają nowego wyboru.

Historia każdej roli jest niezależna od zmian listy sygnatur i odświeżenia podglądu. Dwie role mogą używać wspólnego koloru, ale nie tego samego widocznego komponentu. Kolizja śladów wstrzymuje oba wybory. Do obliczeń trafiają wyłącznie świeże, jednoznaczne wyniki ready. Wybory nie są zapisywane do ustawień.
