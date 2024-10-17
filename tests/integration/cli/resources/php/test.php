<?php
for ($i = 0; $i < 5; $i++) {
    $sqlite = new SQLite3('/db/.ht.sqlite');

    $result = @$sqlite->query("SELECT name FROM sqlite_master WHERE type='table'");

    if ($result) {
        $sqlite->close();
    } else {
        echo "1";
        exit(1);
    };
}

echo "0"
?>
