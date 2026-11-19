# SCh Tomy Podcast: instrukcja

Aplikacja przygotowuje nagrania kazań do podcastu kościoła. Dla każdego pliku:

1. wycina zajawkę serii z początku (od znacznika „start”),
2. przycina ciszę na końcu nagrania,
3. dokleja nasze intro i outro z krótkimi przerwami (po 1 sekundzie),
4. wyrównuje głośność do poziomu podcastów (-16 LUFS),
5. zapisuje gotowy plik MP3 z tytułem i okładką.

Oryginalne nagrania nigdy nie są zmieniane. Gotowy plik ma nazwę oryginału z dopiskiem „-podcast”, na przykład `30-11-2025-kazanie-podcast.mp3`.

Aplikacja działa bez internetu i niczego nie wysyła.

## 1. Pobranie

1. Wejdź na stronę repozytorium na GitHubie i kliknij **Releases** (po prawej stronie).
2. Przy najnowszej wersji rozwiń **Assets** i pobierz plik dla swojego komputera:
   * Windows: `SChTomyPodcast-...-windows-x64-setup.exe`
   * Mac z procesorem Apple (M1, M2, M3, M4): `SChTomyPodcast-...-macos-apple-silicon.dmg`
   * Mac z procesorem Intel: `SChTomyPodcast-...-macos-intel.dmg`

Nie wiesz, jaki masz Mac? Kliknij jabłko w lewym górnym rogu, potem **Ten Mac**. Jeśli przy „Układ” widzisz „Apple M...”, wybierz wersję Apple Silicon. Jeśli przy „Procesor” widzisz „Intel”, wybierz wersję Intel.

## 2. Instalacja na Windows

1. Kliknij dwukrotnie pobrany plik `...-setup.exe`.
2. Pojawi się niebieskie okno „System Windows ochronił ten komputer”. To normalne, bo aplikacja nie ma płatnego podpisu cyfrowego.
3. Kliknij **Więcej informacji**, a potem **Uruchom mimo to**.
4. Przejdź przez instalator (nie potrzebujesz uprawnień administratora).
5. Uruchom aplikację z menu Start: **SCh Tomy Podcast**.

## 3. Instalacja na macOS

1. Kliknij dwukrotnie pobrany plik `.dmg`.
2. Przeciągnij ikonę **SCh Tomy Podcast** do folderu **Aplikacje**.
3. Otwórz folder Aplikacje i kliknij dwukrotnie **SCh Tomy Podcast**.
4. Pojawi się komunikat, że Apple nie może sprawdzić aplikacji. Kliknij **Gotowe** (albo **OK**). To normalne, bo aplikacja nie ma płatnego podpisu Apple.
5. Otwórz **Ustawienia systemowe**, wejdź w **Prywatność i ochrona** i przewiń w dół do sekcji **Ochrona**.
6. Przy informacji o zablokowanej aplikacji „SCh Tomy Podcast” kliknij **Otwórz mimo to** i potwierdź hasłem komputera.
7. W kolejnym okienku kliknij **Otwórz mimo to**. Od teraz aplikacja otwiera się normalnie.

Na starszym macOS (13 Ventura i wcześniejsze) wystarczy: kliknij ikonę aplikacji prawym przyciskiem (albo z klawiszem Control), wybierz **Otwórz**, a potem jeszcze raz **Otwórz**.

Jeśli macOS pisze, że aplikacja „jest uszkodzona i nie można jej otworzyć”, otwórz program **Terminal** (Aplikacje, Narzędzia), wklej poniższą linię i naciśnij Enter, a potem otwórz aplikację ponownie:

```bash
xattr -dr com.apple.quarantine "/Applications/SCh Tomy Podcast.app"
```

## 4. Pierwsze uruchomienie: ustawienia

Kliknij **Ustawienia** w prawym górnym rogu.

* **Intro** i **Outro**: kliknij **Wybierz plik** i wskaż nagranie intro oraz outro. Bez nich aplikacja też działa (przytnie i wyrówna nagrania), ale na górze okna zobaczysz żółte przypomnienie.
* **Okładka (JPG)**: opcjonalnie obrazek, który zobaczą słuchacze.
* **Gdzie zapisać wyniki**: „Obok oryginału” albo wybrany folder.
* Pozostałe wartości są już dobrze ustawione. Nie musisz ich zmieniać.

Kliknij **Zapisz**. Ustawienia zostaną zapamiętane.

## 5. Przygotowanie odcinków krok po kroku

1. **Dodaj nagrania.** Przeciągnij pliki (albo cały folder) na pole „Przeciągnij pliki tutaj” albo kliknij **Wybierz pliki**. Możesz dodać wiele plików naraz.
2. **Poczekaj chwilę na analizę.** Aplikacja szuka początku kazania w każdym pliku. W kolumnie „Początek kazania” zobaczysz czas i jego źródło:
   * **wzorzec zajawki** (zielone): znaleziono zapisaną zajawkę serii, start ustawiono zaraz po niej,
   * **propozycja, sprawdź** (żółte): automatyczna podpowiedź, którą warto sprawdzić,
   * **ręczny** (niebieskie): ustawiony przez Ciebie,
   * **brak**: początek nie zostanie przycięty.
3. **Popraw tytuły.** Tytuł odcinka bierze się z nazwy pliku. Kliknij w pole tytułu i popraw go, jeśli trzeba.
4. **Sprawdź propozycje.** Przy plikach z żółtą propozycją kliknij **Edytuj cięcie** (patrz punkt 6).
5. Kliknij **Przetwórz wszystkie**. Postęp widać przy każdym pliku i nad listą. Możesz w każdej chwili kliknąć **Anuluj obróbkę**.
6. Po zakończeniu w kolumnie „Wynik” zobaczysz głośność (powinna być około -16 LUFS), szczyt, długość i ile sekund ciszy wycięto na końcu. Przycisk **Pokaż w folderze** otwiera folder z gotowym plikiem.

Znak ⚠ przy wyniku oznacza, że głośność odbiega od celu o więcej niż 0,5 LU. Plik jest zapisany, ale warto go odsłuchać.

## 6. Ustawianie początku i końca (Edytuj cięcie)

Okno pokazuje przebieg dźwięku pierwszych 3 minut i ostatnich 2 minut nagrania.

1. Kliknij w przebieg fali, żeby ustawić kursor (biała przerywana linia).
2. Kliknij **▶ Odtwórz od kursora** (albo naciśnij spację), żeby posłuchać.
3. Gdy kursor jest tam, gdzie zaczyna się kazanie, kliknij **Ustaw start w miejscu kursora** (zielona linia „start”).
4. Jeśli na końcu jest coś do wycięcia (na przykład plansza z muzyką), ustaw kursor w dolnym przebiegu i kliknij **Ustaw koniec w miejscu kursora**.
5. Czas możesz też wpisać ręcznie w polach **Start** i **Koniec** w formacie minuty:sekundy,dziesiąte, na przykład `01:23,5`.
6. Kliknij **Zapisz znaczniki**.

Żółta przerywana linia „VAD” to miejsce, w którym aplikacja usłyszała początek dłuższej mowy. Muzyka ze śpiewem może ją zmylić, więc zawsze to odsłuchaj.

## 7. Zapisanie wzorca zajawki serii

Odcinki jednej serii zwykle zaczynają się tą samą zajawką. Wystarczy raz ją zapisać, a aplikacja sama znajdzie ją w kolejnych plikach.

1. Otwórz **Edytuj cięcie** przy pierwszym odcinku serii.
2. Ustaw **start** dokładnie tam, gdzie kończy się zajawka i zaczyna kazanie.
3. Rozwiń **Zapisz zajawkę serii jako wzorzec**.
4. Wpisz nazwę, na przykład nazwę serii kazań. Pola **Od** i **Do** są już wypełnione (od początku nagrania do znacznika start). Popraw je, jeśli zajawka nie zaczyna się od pierwszej sekundy.
5. Kliknij **Zapisz jako zajawkę**.

Pozostałe pliki na liście zostaną od razu sprawdzone ponownie. Przy znalezionej zajawce zobaczysz zieloną etykietę z nazwą wzorca i pewnością dopasowania (np. 92%). Gdy pewność jest niższa niż próg z ustawień (domyślnie 60%), start jest tylko propozycją do sprawdzenia.

Wzorce możesz przejrzeć, zmienić im nazwę albo usunąć w **Ustawieniach**, w sekcji „Zajawki serii”.

## 8. Przerwy i cisza (dla ciekawych)

* Po intro zawsze jest 1 sekunda ciszy, a potem od razu kazanie.
* Jeśli na końcu nagrania jest cisza (na przykład pusta plansza w filmie z YouTube), zostaje z niej 1 sekunda przed outro. Jeśli ciszy nie ma, aplikacja dodaje 1 sekundę.
* Krótkie pauzy w środku kazania nigdy nie są wycinane.
* Długości przerw (od 0 do 3 sekund) i czułość wykrywania ciszy zmienisz w Ustawieniach.

## 9. Gdy coś pójdzie nie tak

* Przy pliku z błędem zobaczysz krótki opis problemu. Przycisk **Szczegóły** pokazuje log techniczny, który możesz skopiować (przycisk **Kopiuj log**) i wysłać osobie, która pomaga przy aplikacji.
* **Spróbuj ponownie** uruchamia obróbkę tego pliku jeszcze raz.
* „Nie udało się odczytać pliku”: sprawdź, czy to na pewno nagranie audio albo wideo i czy plik otwiera się w innym odtwarzaczu.
* „Nie udało się zapisać wyniku”: sprawdź, czy folder wyjściowy istnieje i czy na dysku jest wolne miejsce.
* Jeśli zamkniesz aplikację w trakcie obróbki, niedokończone pliki zostaną usunięte. Oryginały zostają nietknięte.
