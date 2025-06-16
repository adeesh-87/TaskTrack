from fastapi import FastAPI, Depends, HTTPException
from sqlalchemy.orm import Session
import models, schemas
from database import engine, SessionLocal
from fastapi import status

models.Base.metadata.create_all(bind=engine)

app = FastAPI(title="TaskMark - Markdown Task Manager")

# Dependency to get DB session
def get_db():
    db = SessionLocal()
    try:
        yield db
    finally:
        db.close()

@app.get("/notes/", response_model=List[schemas.TaskNoteRead])
def read_notes(db: Session = Depends(get_db)):
    notes = db.query(models.TaskNote).all()
    for note in notes:
        note.tags = note.tags.split(",") if note.tags else []
    return notes


@app.get("/notes/{note_id}", response_model=schemas.TaskNoteRead)
def read_note(note_id: int, db: Session = Depends(get_db)):
    note = db.query(models.TaskNote).filter(models.TaskNote.id == note_id).first()
    if note is None:
        raise HTTPException(status_code=404, detail="Note not found")
    note.tags = note.tags.split(",") if note.tags else []
    return note

@app.delete("/notes/{note_id}", status_code=status.HTTP_204_NO_CONTENT)
def delete_note(note_id: int, db: Session = Depends(get_db)):
    note = db.query(models.TaskNote).filter(models.TaskNote.id == note_id).first()
    if not note:
        raise HTTPException(status_code=404, detail="Note not found")
    
    db.delete(note)
    db.commit()
    return

@app.post("/notes/", response_model=schemas.TaskNoteRead)
def create_note(note: schemas.TaskNoteCreate, db: Session = Depends(get_db)):
    db_note = models.TaskNote(
        title=note.title,
        content=note.content,
        tags=",".join(note.tags)  # Convert list to string
    )
    db.add(db_note)
    db.commit()
    db.refresh(db_note)
    return db_note

@app.put("/notes/{note_id}", response_model=schemas.TaskNoteRead)
def update_note(note_id: int, updated: schemas.TaskNoteCreate, db: Session = Depends(get_db)):
    db_note = db.query(models.TaskNote).filter(models.TaskNote.id == note_id).first()
    if not db_note:
        raise HTTPException(status_code=404, detail="Note not found")

    db_note.title = updated.title
    db_note.content = updated.content
    db_note.tags = ",".join(updated.tags)
    db.commit()
    db.refresh(db_note)
    return db_note
