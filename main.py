
from pathlib import Path
from flask import Flask, render_template, request, send_from_directory, flash, redirect, url_for
from werkzeug.utils import secure_filename

from EmoLabel import EmoLabel

UPLOAD_FOLDER = Path('uploads')
if not UPLOAD_FOLDER.is_dir():
    UPLOAD_FOLDER.mkdir()
ALLOWED_EXTENSIONS = set(['txt', 'csv'])

app = Flask(__name__)
app.config['UPLOAD_FOLDER'] = UPLOAD_FOLDER.absolute().as_posix()

el = EmoLabel()

def allowedFile(filename):
    return '.' in filename and filename.rsplit('.', 1)[1].lower() in ALLOWED_EXTENSIONS


@app.route('/uploads/<filename>')
def uploadedFile(filename):
    return send_from_directory(app.config['UPLOAD_FOLDER'], filename)

# http://flask.pocoo.org/docs/1.0/patterns/fileuploads/
@app.route('/upload', methods=['POST', 'GET'])
def upload():
    if request.method == 'POST':
        # check if the post request has the file part
        if 'file' not in request.files:
            flash('No file part')
            return redirect(request.url)
        file = request.files['file']
        # if user does not select file, browser also
        # submit a empty part without filename
        if file.filename == '':
            flash('No selected file')
            return redirect(request.url)
        if file and allowedFile(file.filename):
            filename = secure_filename(file.filename)
            fileSavePath = app.config['UPLOAD_FOLDER'] + '/' + filename
            file.save(fileSavePath)
            # TODO
            # Insert data to db.
            return redirect(url_for('uploadedFile', filename=filename))
    return render_template('upload.html')

@app.route('/', methods=['POST', 'GET'])
def index():
    
    if request.method == 'POST':

        idx = int(request.form.get('idx'))
        
        score = int(request.form.get('score'))
        
        tag = str(request.form.get('tag'))
        
        tag_opt = list()
        for item in eval(str(request.form)[19:-1]):
            if item[0] == 'tag_opt':
                tag_opt.append(item[1])

        
        el.insertLabel(idx, score, tag, tag_opt)
        el.commit()
        print(idx, score, tag, tag_opt)

    idx, text, date = el.randomSampleText()
    customTags = el.getUserCustomLabelById(idx)

    return render_template('index.html', idx=idx, text=text, customTags=customTags)


if __name__ == '__main__':

    app.run(host='0.0.0.0', port='8888', debug=True)