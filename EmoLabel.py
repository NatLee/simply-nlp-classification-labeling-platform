import sqlite3
import numpy as np
from collections import defaultdict
from datetime import datetime
from random import randint, choice

class EmoLabel(object):

    def __init__(self, dbPath:str='emoLabel.db'):
        self.__conn = sqlite3.connect(dbPath, check_same_thread=False)
        self.__cur = self.__conn.cursor()
        self.__displayTextIds = self.__calDisplayTextIds()
        self.updateTicketNumber()

    def reloadTextIds(self):
        self.__displayTextIds = self.__calDisplayTextIds()
        return


    def updateTicketNumber(self):

        # Create votebox on first execute.
        if not hasattr(self, 'voteBox'):
            self.voteBox = dict()
        
        # Load all IDs from database.
        
        ids = self.__displayTextIds
        
        # Create a dict that holds the current count of all entries, returns 0 if entry doesn't exists.
        currentVote = defaultdict(lambda: 0, 
            self.__cur.execute(
                'SELECT "emotionId", COUNT(*) FROM "label" GROUP BY "emotionId"'
                ).fetchall())

        # Update vote count of all IDs
        for entry in ids:
            self.voteBox[entry] = currentVote[entry]


    def __calDisplayTextIds(self) -> list:
        '''
        Return a list of flag=1 text.
        '''
        results = self.__cur.execute('select id from emotion where flag=1').fetchall()
        displayTextIds = [result[0] for result in results]
        return displayTextIds

    def randomSampleText(self, displayFlag:int=1) -> (int, str, str):
        '''
        Random choose a text to label.
        '''
        selectIdx = self.__selectLogic(self.__displayTextIds)
        query = 'select id, text, date from emotion where id=? and flag=?'
        paras = (str(selectIdx), str(displayFlag))
        idx, text, date = self.__cur.execute(query, paras).fetchone()
        return idx, text, date

    def dataDashboard(self):
        '''
        Current votebox status(Mean, STD).
        '''
        counter = np.asarray(list(self.voteBox.values()))
        return (np.sum(counter)/len(counter), np.std(counter))

    def __selectLogic(self, candidateID: list, scaleFactor=1) -> int:
        '''
        Weighted pick from list
        '''

        # Ticket weight rule:
        #   LET vote as list of entries vote count 
        #   FOR EVERY ENTRY e
        #   ticket count = (MAX(votes) - votes[e] + 1) * scale
        
        tickets = []
        maxVote = max(self.voteBox.values())

        for entryId in candidateID:
            tickets += [entryId] * int((maxVote - self.voteBox[entryId] + 1) * scaleFactor)
            
        return choice(tickets)

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

        # Update voteBox.
        self.voteBox[emotionId] = self.voteBox[emotionId] + 1
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

