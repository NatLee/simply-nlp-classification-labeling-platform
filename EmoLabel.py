import sqlite3
from datetime import datetime
from random import randint

class EmoLabel(object):

    def __init__(self, dbPath:str='emoLabel.db'):
        self.__conn = sqlite3.connect(dbPath, check_same_thread=False)
        self.__cur = self.__conn.cursor()
        self.__displayTextIds = self.__calDisplayTextIds()

    def reloadTextIds(self):
        self.__displayTextIds = self.__calDisplayTextIds()
        return

    def __calDisplayTextIds(self) -> list:
        '''
        Return a list of flag=1 text.
        '''
        results = self.__cur.execute('select id from emotion where flag=1').fetchall()
        displayTextIds = [result[0] for result in results]
        return displayTextIds

    def randomSampleText(self, displayFlag:int=1) -> (int, str, str):
        randomNumber = self.__selectLogic(len(self.__displayTextIds))
        selectIdx = self.__displayTextIds[randomNumber]
        query = 'select id, text, date from emotion where id=? and flag=?'
        paras = (str(selectIdx), str(displayFlag))
        idx, text, date = self.__cur.execute(query, paras).fetchone()
        return idx, text, date

    def __selectLogic(self, length:int) -> int:
        '''
        Select logic here.
        '''
        return randint(1, length-1)

    def insertText(self, text:str):
        date = datetime.now().strftime('%Y-%m-%d %H:%M:%S')
        query = 'insert or ignore into emotion (text, date, flag) values (?, ?, ?)'
        paras = (text, str(date), 1)
        self.__cur.execute(query, paras)
        return

    def insertLabel(self, emotionId:int, score:int, tag:str, textType:str ,tag_opt:str):
        date = datetime.now().strftime('%Y-%m-%d %H:%M:%S')
        query = 'insert into label (emotionId, score, tag, type, tag_opt, updateDate) values (?, ?, ?, ?, ?, ?)'
        paras = (str(emotionId), str(score), str(tag), str(textType), tag_opt.__repr__(), str(date))        
        self.__cur.execute(query, paras)
        return

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

