
from flask import Flask, render_template, request
from EmoLabel import EmoLabel
from datetime import datetime

app = Flask(__name__)

el = EmoLabel()


@app.route('/', methods=['POST', 'GET'])
def index():
    
    if request.method == 'POST':
        idx = int(request.form.get('idx'))
        score = int(request.form.get('score'))
        tag = str(request.form.get('tag'))
        tag_opt = request.form.get('tag_opt')
        date = datetime.now().strftime('%Y-%m-%d %H:%M:%S')
        
        print(idx, score, tag, tag_opt, date)

    idx, text, date = el.randomSampleText()
    return render_template('index.html', idx=idx, text=text)


if __name__ == '__main__':

    app.run(host='0.0.0.0', port='8888', debug=True)