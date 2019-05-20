import sqlite3
from datetime import datetime
from random import randint

class EmoLabel(object):

    def __init__(self, dbPath:str='emoLabel.db'):
        self.__conn = sqlite3.connect(dbPath, check_same_thread=False)
        self.__cur = self.__conn.cursor()
        self.__textCounts = self.__calTextCounts()

    def __calTextCounts(self) -> int:
        result = self.__cur.execute('select count(*) from emotion').fetchone()[0]
        return result

    def refreashTextCounts(self):
        self.__textCounts = self.__calTextCounts()


    def randomSampleText(self) -> (int, str, str):
        randomNumber = randint(1, self.__textCounts)
        idx, text, date = self.__cur.execute('select * from emotion where id=' + str(randomNumber)).fetchone()
        return idx, text, date

    def insertText(self, text:str):
        date = datetime.now().strftime('%Y-%m-%d %H:%M:%S')
        query = 'insert into emotion (text, date) values (?, ?)'
        paras = (text, str(date))
        self.__cur.execute(query, paras)
        return

    def insertLabel(self, emotionId:int, score:int, tag:str, tag_opt:str):
        date = datetime.now().strftime('%Y-%m-%d %H:%M:%S')
        query = 'insert into label (emotionId, score, tag, tag_opt, updateDate) values (?, ?, ?, ?, ?)'
        paras = (str(emotionId), str(score), str(tag), tag_opt.__repr__(), str(date))        
        self.__cur.execute(query, paras)

    def getUserCustomLabelById(self, emotionId:int) -> list:
        results = self.__cur.execute('select tag_opt from label where emotionId =' + str(emotionId)).fetchall()
        tagList = list()
        for result in results:
            tagList = tagList + eval(result[0])
        return list(set(tagList))

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

