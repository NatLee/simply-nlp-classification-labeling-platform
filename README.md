# Labe Master


## Database

Use sqlite.

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
);
```
## Usage

`python main.py`

And, it will serve on `0.0.0.0:5000`

## Page

![](./doc/page.png)
