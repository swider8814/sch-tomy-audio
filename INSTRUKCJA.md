# SCh Tomy Audio: instrukcja

Aplikacja przygotowuje nagrania kazań do podcastu kościoła. Dla każdego pliku:

1. wycina zajawkę serii z początku (od znacznika „start”),
2. przycina ciszę na końcu nagrania,
3. dokleja nasze intro i outro z krótkimi przerwami (po 1 sekundzie),
4. wyrównuje głośność do poziomu podcastów (-16 LUFS),
5. zapisuje gotowy plik MP3 z tytułem i okładką.

Oryginalne nagrania nigdy nie są zmieniane. Gotowy plik ma nazwę z daty, tytułu i mówcy z dopiskiem „-podcast”, na przykład `2026-10-11-co-mi-wolno-adam-szumorek-podcast.mp3`. Gdy tych danych nie ma, nazwa powstaje z nazwy oryginału.

Aplikacja działa bez internetu i niczego o Tobie nie wysyła. Internetu używa tylko w dwóch sytuacjach: przy pobieraniu nagrania z YouTube, gdy sam wkleisz link, oraz przy sprawdzaniu, czy jest nowa wersja aplikacji (patrz punkt 10).

## 1. Pobranie

1. Wejdź na stronę repozytorium na GitHubie i kliknij **Releases** (po prawej stronie).
2. Przy najnowszej wersji rozwiń **Assets** i pobierz plik dla swojego komputera:
   * Windows: `SChTomyAudio-...-windows-x64-setup.exe`
   * Mac z procesorem Apple (M1, M2, M3, M4): `SChTomyAudio-...-macos-apple-silicon.dmg`
   * Mac z procesorem Intel: `SChTomyAudio-...-macos-intel.dmg`

Nie wiesz, jaki masz Mac? Kliknij jabłko w lewym górnym rogu, potem **Ten Mac**. Jeśli przy „Układ” widzisz „Apple M...”, wybierz wersję Apple Silicon. Jeśli przy „Procesor” widzisz „Intel”, wybierz wersję Intel.

**Zmiana nazwy w wersji 0.1.3.** Do wersji 0.1.2 aplikacja nazywała się „SCh Tomy Podcast”, od wersji 0.1.3 nazywa się „SCh Tomy Audio”. Nowa wersja instaluje się jako osobna aplikacja, więc starą odinstaluj. Ustawienia, intro, outro, zapisane zajawki i lista plików zostaną zachowane.

* **Windows:** otwórz **Ustawienia**, potem **Aplikacje**, znajdź **SCh Tomy Podcast** i kliknij **Odinstaluj**. Nie zaznaczaj usuwania danych aplikacji, jeśli instalator o to zapyta.
* **Mac:** w folderze **Aplikacje** przeciągnij **SCh Tomy Podcast** do Kosza.

## 2. Instalacja na Windows

1. Kliknij dwukrotnie pobrany plik `...-setup.exe`.
2. Pojawi się niebieskie okno „System Windows ochronił ten komputer”. To normalne, bo aplikacja nie ma płatnego podpisu cyfrowego.
3. Kliknij **Więcej informacji**, a potem **Uruchom mimo to**.
4. Przejdź przez instalator (nie potrzebujesz uprawnień administratora).
5. Uruchom aplikację z menu Start: **SCh Tomy Audio**.

## 3. Instalacja na macOS

1. Kliknij dwukrotnie pobrany plik `.dmg`.
2. Przeciągnij ikonę **SCh Tomy Audio** do folderu **Aplikacje**.
3. Otwórz folder Aplikacje i kliknij dwukrotnie **SCh Tomy Audio**.
4. Pojawi się komunikat, że Apple nie może sprawdzić aplikacji. Kliknij **Gotowe** (albo **OK**). To normalne, bo aplikacja nie ma płatnego podpisu Apple.
5. Otwórz **Ustawienia systemowe**, wejdź w **Prywatność i ochrona** i przewiń w dół do sekcji **Ochrona**.
6. Przy informacji o zablokowanej aplikacji „SCh Tomy Audio” kliknij **Otwórz mimo to** i potwierdź hasłem komputera.
7. W kolejnym okienku kliknij **Otwórz mimo to**. Od teraz aplikacja otwiera się normalnie.

Na starszym macOS (13 Ventura i wcześniejsze) wystarczy: kliknij ikonę aplikacji prawym przyciskiem (albo z klawiszem Control), wybierz **Otwórz**, a potem jeszcze raz **Otwórz**.

Jeśli macOS pisze, że aplikacja „jest uszkodzona i nie można jej otworzyć”, otwórz program **Terminal** (Aplikacje, Narzędzia), wklej poniższą linię i naciśnij Enter, a potem otwórz aplikację ponownie:

```bash
xattr -dr com.apple.quarantine "/Applications/SCh Tomy Audio.app"
```

## 4. Pierwsze uruchomienie: ustawienia

Kliknij **Ustawienia** w prawym górnym rogu. Ustawienia są podzielone na pięć zakładek: **Podstawowe**, **Dźwięk**, **Zajawki i zaawansowane**, **Skróty klawiszowe** oraz **O programie**. Na początek wystarczy zakładka **Podstawowe**:

* **Intro** i **Outro**: kliknij **Wybierz plik** i wskaż nagranie intro oraz outro. Bez nich aplikacja też działa (przytnie i wyrówna nagrania), ale na górze okna zobaczysz żółte przypomnienie.
* **Okładka (JPG)**: opcjonalnie obrazek, który zobaczą słuchacze.
* **Gdzie zapisać gotowe pliki**: „Obok oryginału” albo wybrany folder.
* **Nagrania z YouTube**: folder, do którego trafia dźwięk pobrany z YouTube (domyślnie Pobrane/SCh Tomy YouTube). Przycisk **Domyślny** przywraca ten folder.
* **Obserwowany folder** (opcjonalnie): wskaż folder, do którego trafiają nowe nagrania, np. z komputera nagrywającego nabożeństwo albo z dysku sieciowego. Każde nowe nagranie, które się w nim pojawi, aplikacja sama doda do listy, gdy plik skończy się kopiować. Pliki, które już są w folderze w chwili jego wyboru, nie są dodawane. Działa też wtedy, gdy nagranie pojawiło się przy wyłączonej aplikacji: zostanie dodane przy następnym uruchomieniu. Przycisk **Wyłącz** wyłącza obserwowanie.
* **Powiadomienia**: gdy obróbka wszystkich plików się skończy, a okno aplikacji nie jest na wierzchu, system pokaże powiadomienie. Przy pierwszej obróbce Windows albo macOS może zapytać o zgodę na powiadomienia.
* **Wygląd**: „Jak w systemie” (aplikacja jest jasna albo ciemna tak jak Windows czy macOS), „Jasny” albo „Ciemny”. Wygląd zmienisz też szybko ikonką na górnym pasku, obok „Ustawienia”: księżyc włącza ciemny wygląd, słońce jasny. Żeby wrócić do „Jak w systemie”, wybierz tę opcję w Ustawieniach.
* **Tagi MP3**: wykonawca i album zapisywane w każdym pliku.
* W zakładce **Dźwięk** są przerwy, cisza, poprawa dźwięku mowy, głośność i format MP3, a w zakładce **Zajawki i zaawansowane** zapisane zajawki serii, zapamiętani mówcy, przeniesienie ustawień na inny komputer i liczba plików obrabianych naraz. Te wartości są już dobrze ustawione. Nie musisz ich zmieniać.

Kliknij **Zapisz**. Ustawienia zostaną zapamiętane.

## 5. Przygotowanie odcinków krok po kroku

1. **Dodaj nagrania.** Przeciągnij pliki (albo cały folder) w dowolne miejsce okna aplikacji albo kliknij **Wybierz pliki**. Możesz dodać wiele plików naraz.
2. **Poczekaj chwilę na analizę.** Aplikacja szuka początku kazania w każdym pliku. W kolumnie „Początek kazania” zobaczysz czas i jego źródło:
   * **wzorzec: nazwa** (zielone): znaleziono zapisaną zajawkę serii, start ustawiono zaraz po niej,
   * **propozycja, sprawdź** (żółte): automatyczna podpowiedź, którą warto sprawdzić,
   * **ręczny** (niebieskie): ustawiony przez Ciebie,
   * **bez cięcia**: początek nie zostanie przycięty.

   Gdy zatrzymasz kursor myszy nad etykietą, zobaczysz więcej szczegółów (np. pewność dopasowania zajawki).
3. **Wpisz tytuły kazań.** Przy każdym pliku są trzy pola:
   * **Tytuł kazania** (większe pole), np. „Co mi wolno?”. Jeśli tytuł jest w nazwie pliku (np. „06.09.2026 - Rafał Piekarski - Wspólniej_audio.mp3”), aplikacja wpisze go sama. Puste pole jest podświetlone na żółto. Klawisz **Enter** przenosi Cię do tytułu następnego pliku, więc tytuły możesz wpisywać jeden po drugim.
   * **Mówca** i **data** (dwa mniejsze pola pod spodem), np. „Adam Szumorek” i „2026.10.11”. Aplikacja odczytuje je z nazwy pliku (a przy nagraniach z YouTube z opisu filmu), możesz je poprawić. Puste pole mówcy lub daty jest podświetlone na żółto. Po kliknięciu w pole mówcy rozwinie się lista zapamiętanych mówców. Wystarczy wpisać 2 lub 3 pierwsze litery imienia albo nazwiska (np. „pie”), a lista pokaże tylko pasujące osoby; naciśnij **Enter**, żeby wybrać pierwszą, strzałki, żeby wybrać inną, albo kliknij nazwisko. Możesz też wpisać nowe imię i nazwisko. Datę wpisz jako RRRR.MM.DD (np. 2026.11.01); aplikacja zrozumie też 1.11.2026 i sama ją poprawi. Gdy nazwisko wygląda na literówkę (np. „Szumorke” zamiast „Szumorek”), pod polem pojawi się pytanie „Czy chodziło o...?” z przyciskiem **Popraw**. Nazwisko z literówką nie trafia do listy zapamiętanych mówców, jeśli na liście jest już poprawna wersja. Listę zapamiętanych mówców (i usuwanie z niej literówek z wcześniejszych wersji) znajdziesz w Ustawieniach, w zakładce „Zajawki i zaawansowane”. Tam też, pod **Dodaj listę mówców**, wkleisz od razu wiele nazwisk (np. ze strony kościoła, każde w osobnym wierszu) albo wczytasz je z pliku .txt.

   Gdy zatrzymasz kursor myszy nad małym znakiem **i** obok, zobaczysz, jaki tytuł trafi do pliku MP3 („Co mi wolno? - Adam Szumorek - 2026.10.11”, tak jak w Spotify) i jak będzie się nazywał gotowy plik (`2026-10-11-co-mi-wolno-adam-szumorek-podcast.mp3`: bez polskich znaków i spacji, z datą na początku, żeby pliki układały się po kolei). Tytuł odcinka w Spotify ustawiasz osobno, we wpisie w WordPressie, najlepiej taki sam.
4. **Sprawdź propozycje.** Przy plikach z żółtą propozycją kliknij **Sprawdź początek** (patrz punkt 6). W oknie cięcia przycisk **Zatwierdź i następny** zapisuje zmiany i od razu otwiera kolejny plik do sprawdzenia.
   Nad listą są filtry: **Wszystkie**, **Do sprawdzenia**, **Bez tytułu**, **Błędy** i **Gotowe**. Liczba w nawiasie mówi, ile plików pasuje. Kliknij filtr, żeby zobaczyć tylko te pliki.
5. Kliknij **Przetwórz wszystkie**. Postęp widać przy każdym pliku i nad listą. Możesz w każdej chwili kliknąć **Anuluj obróbkę**.
6. Po zakończeniu w kolumnie „Wynik” zobaczysz głośność (powinna być około -16 LUFS), szczyt, długość i ile sekund ciszy wycięto na końcu. Przyciski **▶ Początek** i **▶ Koniec** odtwarzają w aplikacji początek gotowego odcinka (intro, przerwa i początek kazania) albo jego koniec (koniec kazania, przerwa i outro), żeby szybko sprawdzić przejścia. Przycisk **Pokaż w folderze** otwiera folder z gotowym plikiem.

   Jeśli dodasz nagranie, które było już kiedyś przetworzone (także kopię pod inną nazwą albo ten sam film z YouTube), w kolumnie „Stan” pojawi się ostrzeżenie „To nagranie przetworzono już ...” z datą. Plik nadal możesz przetworzyć, ostrzeżenie chroni tylko przed przypadkową podwójną publikacją.

   W trakcie obróbki nad listą widać, ile plików jest gotowych i mniej więcej ile czasu zostało do końca.

Lista plików jest zapamiętywana: po zamknięciu i ponownym otwarciu aplikacji pliki wracają na listę (pliki przerwane w trakcie obróbki czekają na ponowne **Przetwórz wszystkie**, a przerwane pobieranie z YouTube wznowi się samo). Podczas obróbki i pobierania aplikacja nie pozwala komputerowi zasnąć.

Znak ⚠ przy wyniku oznacza, że głośność odbiega od celu o więcej niż 0,5 LU. Plik jest zapisany, ale warto go odsłuchać.

## 5a. Nagrania z YouTube

1. Kliknij **Pobierz z YouTube** (obok „Wybierz pliki”).
2. Wklej adres filmu (np. `https://www.youtube.com/watch?v=...` albo `https://youtu.be/...`). Możesz wkleić kilka adresów naraz, każdy w osobnej linii.
3. Kliknij **Pobierz**. Aplikacja pobierze sam dźwięk (bez obrazu), film po filmie, do folderu z Ustawień (domyślnie **Pobrane/SCh Tomy YouTube**) i doda nagrania do listy. Tytuł kazania weźmie z tytułu filmu. Mówcę i datę odczyta z opisu filmu: mówcę z Twojej listy mówców, a gdy go tam nie ma, z takiego zdania jak „wprowadził nas Jerzy Sikora” lub „Mówca: Jerzy Sikora”, datę z wiersza „Data: 01.11.2026” (gdy go nie ma, przyjmie dzień publikacji filmu). Sprawdź oba pola; jeśli mówcy nie udało się odczytać, pole jest podświetlone na żółto i wybierzesz go z listy.
4. Dalej wszystko działa tak samo jak dla plików: sprawdź start, kliknij **Przetwórz wszystkie**.

**Lista linków w pliku.** Jeśli masz plik tekstowy (.txt) z wieloma linkami, kliknij w tym samym okienku **Wczytaj listę z pliku (.txt)** albo po prostu przeciągnij ten plik na okno aplikacji. W pliku każdy link jest w osobnej linii; puste linie i linie zaczynające się od `#` (np. `# Piotrków 2025`) są pomijane, powtórzone linki też.

Z YouTube aplikacja łączy się tylko wtedy, gdy sam wkleisz link. Jeśli pobieranie się nie uda („YouTube zablokował pobieranie”), spróbuj później albo pobierz plik w inny sposób i przeciągnij go do aplikacji. Przy błędzie przy nagraniu jest przycisk **Spróbuj ponownie**. YouTube czasem zmienia sposób działania; wtedy pomaga nowa wersja aplikacji.

## 6. Ustawianie początku i końca (Sprawdź początek / Edytuj cięcie)

Okno pokazuje przebieg dźwięku pierwszych 3 minut nagrania (dłużej, jeśli propozycja startu wypada później) i ostatnich 2 minut.

1. Kliknij w przebieg fali, żeby ustawić kursor (biała przerywana linia).
2. Kliknij **▶ Odtwórz od kursora** (albo naciśnij spację), żeby posłuchać.
3. Gdy kursor jest tam, gdzie zaczyna się kazanie, kliknij **Ustaw start w miejscu kursora** (zielona linia „start”).
4. Jeśli na końcu jest coś do wycięcia (na przykład plansza z muzyką), ustaw kursor w dolnym przebiegu i kliknij **Ustaw koniec w miejscu kursora**.
5. Czas możesz też wpisać ręcznie w polach **Start** i **Koniec** w formacie minuty:sekundy,dziesiąte, na przykład `01:23,5`.
6. Na górze okna możesz od razu wpisać albo poprawić **Tytuł kazania**.
7. Kliknij **Zatwierdź i następny**, żeby zapisać i przejść do kolejnego pliku, który ma tylko propozycję startu albo nie ma tytułu. Gdy takich plików już nie ma, kliknij **Zapisz i zamknij**.

**Powiększanie.** Kółko myszy nad przebiegiem fali przybliża i oddala widok wokół wskaźnika myszy (z klawiszem Shift przewija w bok). Możesz też użyć przycisków **Przybliż**, **Oddal** i **Całość**.

**Skróty klawiszowe** (gdy nie piszesz w polu tekstowym):

* **spacja**: odtwórz od kursora albo zatrzymaj,
* **strzałka w lewo / w prawo**: przesuń kursor o 0,1 sekundy (z klawiszem Shift o 1 sekundę),
* **S**: ustaw start w miejscu kursora,
* **E**: ustaw koniec w miejscu kursora dolnego przebiegu,
* **+** i **-**: przybliż i oddal.

Żółta przerywana linia „VAD” to miejsce, w którym aplikacja usłyszała początek dłuższej mowy. Muzyka ze śpiewem może ją zmylić, więc zawsze to odsłuchaj.

## 7. Zapisanie wzorca zajawki serii

Odcinki jednej serii zwykle zaczynają się tą samą zajawką. Wystarczy raz ją zapisać, a aplikacja sama znajdzie ją w kolejnych plikach.

1. Otwórz okno cięcia (**Edytuj cięcie** albo **Sprawdź początek**) przy pierwszym odcinku serii.
2. Ustaw **start** dokładnie tam, gdzie kończy się zajawka i zaczyna kazanie.
3. Rozwiń **Zapisz zajawkę serii jako wzorzec**.
4. Wpisz nazwę, na przykład nazwę serii kazań. Pola **Od** i **Do** są już wypełnione (od początku nagrania do znacznika start). Popraw je, jeśli zajawka nie zaczyna się od pierwszej sekundy.
5. Kliknij **Zapisz jako zajawkę**.

Pozostałe pliki na liście zostaną od razu sprawdzone ponownie. Przy znalezionej zajawce zobaczysz zieloną etykietę z nazwą wzorca i pewnością dopasowania (np. 92%). Gdy pewność jest niższa niż próg z ustawień (domyślnie 60%), start jest tylko propozycją do sprawdzenia.

Wzorce możesz przejrzeć, zmienić im nazwę albo usunąć w **Ustawieniach**, w zakładce „Zajawki i zaawansowane”.

## 8. Przerwy i cisza (dla ciekawych)

* Po intro zawsze jest 1 sekunda ciszy, a potem od razu kazanie.
* Jeśli na końcu nagrania jest cisza (na przykład pusta plansza w filmie z YouTube), zostaje z niej 1 sekunda przed outro. Jeśli ciszy nie ma, aplikacja dodaje 1 sekundę.
* Krótkie pauzy w środku kazania nigdy nie są wycinane.
* Długości przerw (od 0 do 3 sekund) i czułość wykrywania ciszy zmienisz w Ustawieniach, w zakładce „Dźwięk”.
* **Poprawa dźwięku mowy** (zakładka „Dźwięk”, domyślnie wyłączona) usuwa buczenie i niskie dudnienie (np. od nagłośnienia albo stuknięć w mikrofon) i wycisza stały szum w tle. Działa tylko na nagranie kazania, intro i outro zostają bez zmian. Włącz ją, gdy nagrania mają szum sali albo buczenie, i odsłuchaj efekt przyciskiem **▶ Początek** przy gotowym pliku.

## 9. Gdy coś pójdzie nie tak

* Przy pliku z błędem zobaczysz krótki opis problemu. Przycisk **Szczegóły** pokazuje log techniczny, który możesz skopiować (przycisk **Kopiuj log**) i wysłać osobie, która pomaga przy aplikacji.
* **Spróbuj ponownie** uruchamia obróbkę tego pliku jeszcze raz.
* „Nie udało się odczytać pliku”: sprawdź, czy to na pewno nagranie audio albo wideo i czy plik otwiera się w innym odtwarzaczu.
* „Nie udało się zapisać wyniku”: sprawdź, czy folder wyjściowy istnieje i czy na dysku jest wolne miejsce.
* Jeśli zamkniesz aplikację w trakcie obróbki, niedokończone pliki zostaną usunięte. Oryginały zostają nietknięte.
* Kontakt z autorem aplikacji (Daniel Świderski, daniel.swiderski@proem.pl) znajdziesz w **Ustawieniach**, w zakładce **O programie**. Przy zgłaszaniu problemu podaj numer wersji z tej zakładki i dołącz log.

## 10. Nowa wersja aplikacji

Przy uruchomieniu aplikacja sprawdza na GitHubie, czy jest nowsza wersja. Jeśli jest, na górze okna pojawi się niebieski pasek „Jest nowa wersja aplikacji”. Kliknij **Pobierz nową wersję**: w przeglądarce otworzy się strona wydania, skąd pobierasz instalator tak samo jak za pierwszym razem (punkt 1). Twoje ustawienia, wzorce zajawek i lista plików zostaną zachowane.

Numer Twojej wersji, przycisk **Sprawdź teraz** i wyłącznik automatycznego sprawdzania znajdziesz w **Ustawieniach**, w zakładce **O programie**. Aplikacja sama niczego nie pobiera ani nie instaluje.

## 11. Drugi komputer: przeniesienie ustawień

Jeśli odcinki przygotowuje więcej osób, nie trzeba wszystkiego ustawiać od nowa.

1. Na komputerze, na którym wszystko jest ustawione, otwórz **Ustawienia**, zakładkę **Zajawki i zaawansowane**, i kliknij **Zapisz ustawienia do pliku**. Powstanie jeden plik z końcówką `.schtomy`.
2. Przenieś ten plik na drugi komputer (np. pendrive'em albo e-mailem).
3. Na drugim komputerze w tym samym miejscu kliknij **Wczytaj ustawienia z pliku** i wskaż plik.

W pliku są intro, outro, okładka, zajawki serii, zapamiętani mówcy i pozostałe ustawienia. Foldery (na gotowe pliki, na nagrania z YouTube i obserwowany) zostają takie, jak były na drugim komputerze, bo na każdym komputerze mogą być inne. Zajawki i mówców aplikacja dopisze do tych, które już tam są.
