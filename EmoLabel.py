import sqlite3
from random import randint

class EmoLabel(object):

    def __init__(self, dbPath:str='emoLabel.db'):
        self.__conn = sqlite3.connect(dbPath, check_same_thread=False)
        self.__cur = self.__conn.cursor()
        self.__textCounts = self.__calTextCounts()

    def __calTextCounts(self) -> int:
        result = self.__cur.execute('select count(*) from emotion').fetchone()[0]
        return result

    def randomSampleText(self) -> (int, str, str):
        randomNumber = randint(1, self.__textCounts)
        idx, text, date = self.__cur.execute('select * from emotion where id=' + str(randomNumber)).fetchone()
        return idx, text, date

    def insertText(self, text:str):
        self.__cur.execute('insert into emotion (text) values ("' + str(text) +'")')
        return

    def insertLabel(self, emotionId:int, score:int, tag:str, tag_opt:str):
        self.__cur.execute('insert into label (emotionId, score, tag, tag_opt) values ("' + str(emotionId)      + ', ' +
                     str(score)          + ', "' +
                     tag.__repr__()      + '", "' +
                     tag_opt.__repr__()  + '")')

    def getText(self) -> list:
        results = self.__cur.execute('select * from emotion').fetchall()
        return results


    def commit(self):
        self.__conn.commit()
        return

    def close(self):
        self.__conn.close()
        return

el = EmoLabel()

