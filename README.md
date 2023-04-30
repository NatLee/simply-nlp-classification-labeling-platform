# Simply NLP Classification Labeling Platform

<p style="text-align: center">
  <img align="center" src="https://raw.githubusercontent.com/NatLee/simply-nlp-classification-labeling-platform/main/doc/demo.gif" alt="Demo">
</p>

This is a flask-based simply platform for labeling classification of a text.

The platform is designed to help users create labeled datasets for training classification models in natural language processing (NLP) tasks.

> Testing source is from [chinese_sentiment](https://github.com/sweslo17/chinese_sentiment).

## Database Schema

We all love to use a very useful sqlite-based database.

```sql
CREATE TABLE "emotion" (
	"id"	INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT UNIQUE,
	"text"	INTEGER NOT NULL UNIQUE,
	"date"	TEXT NOT NULL,
	"flag"	INTEGER NOT NULL
);


CREATE TABLE "label" (
	"id"	INTEGER NOT NULL PRIMARY KEY AUTOINCREMENT UNIQUE,
	"emotionId"	INTEGER NOT NULL,
	"score"	REAL NOT NULL,
	"tag"	TEXT NOT NULL,
	"type"	TEXT NOT NULL,
	"tag_opt"	TEXT NOT NULL,
	"updateDate"	TEXT NOT NULL
	FOREIGN KEY("emotionId") REFERENCES "emotion"("id")
);
```

## Usage

```bash
pip install -r requirements.txt
python main.py
```

And, it will serve on `http://localhost:5000`

## Contributors

<!-- ALL-CONTRIBUTORS-LIST:START - Do not remove or modify this section -->
<!-- prettier-ignore-start -->
<!-- markdownlint-disable -->
<table>
  <tbody>
    <tr>
      <td align="center"><a href="https://github.com/NatLee"><img src="https://avatars.githubusercontent.com/u/10178964?v=3?s=100" width="100px;" alt="Nat Lee"/><br /><sub><b>Nat Lee</b></sub></a></td>
      <td align="center"><a href="https://github.com/h-alice"><img src="https://avatars.githubusercontent.com/u/16372174?v=3?s=100" width="100px;" alt="H-Alice"/><br /><sub><b>H-Alice</b></sub></a></td>
    </tr>
  </tbody>
</table>

<!-- markdownlint-restore -->
<!-- prettier-ignore-end -->

<!-- ALL-CONTRIBUTORS-LIST:END -->

