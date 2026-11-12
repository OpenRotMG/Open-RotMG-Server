/*
 * behavior.cc
 *
 *  Created on: Dec 31, 2009
 *      Author: Rob Shillingsburg
 */

#include "behavior.h"
#include "gameobject.h"
#include "game.h"
#include "person.h"
#include "constants.h"
#include "visibility.h"
#include "application.h"
#include "stringlists.h"
#include "inventory.h"
#include "xml.h"
#include "stats.h"
#include "state.h"
#include "census.h"
#include "mapparser.h"

using std::stringstream;

/*static*/
unordered_map<string, BehaviorCreatorBase*>* Behavior::behaviorNameMap_ = NULL;

struct BehaviorCreatorBase {
	virtual Behavior* create(GameObject *host,
	const XMLOptionsMap &options) = 0;
};

template<typename T>
struct BehaviorCreator : public BehaviorCreatorBase {
	bool onlyInRealm_;

	BehaviorCreator(bool onlyInRealm) {
		onlyInRealm_ = onlyInRealm;
	}

	virtual Behavior* create(GameObject *host,
		const XMLOptionsMap &options) {
		if (onlyInRealm_ && host->game()->gameType() != REALM_GAME) {
			return NULL;
		}
		return new T(host, options);
	}
};

Behavior::Behavior(GameObject *host, const XMLOptionsMap& options) :
host_(host), closed_(false) {

	bucket_ = GetStringArgument(options, "bucket", "");
}

Behavior *Behavior::New(const XMLOptionedTag &kind, GameObject *host) {
	if (behaviorNameMap_ == NULL) {
		behaviorNameMap_ = new unordered_map<string, BehaviorCreatorBase*>;
		behaviorNameMap_->insert(make_pair("Shoot",
			new BehaviorCreator<ShootBehavior>(false)));
		behaviorNameMap_->insert(make_pair("Wander",
			new BehaviorCreator<WanderBehavior>(false)));
		behaviorNameMap_->insert(make_pair("Charge",
			new BehaviorCreator<ChargeBehavior>(false)));
		behaviorNameMap_->insert(make_pair("Follow",
			new BehaviorCreator<FollowBehavior>(false)));
		behaviorNameMap_->insert(make_pair("ChangeSize",
			new BehaviorCreator<ChangeSizeBehavior>(false)));
		behaviorNameMap_->insert(make_pair("Metamorphose",
			new BehaviorCreator<MetamorphoseBehavior>(false)));
		behaviorNameMap_->insert(make_pair("MetamorphoseAtDeath",
			new BehaviorCreator<MetamorphoseAtDeathBehavior>(false)));
		behaviorNameMap_->insert(make_pair("Explode",
			new BehaviorCreator<ExplodeBehavior>(false)));
		behaviorNameMap_->insert(make_pair("Protect",
			new BehaviorCreator<ProtectBehavior>(false)));
		behaviorNameMap_->insert(make_pair("VictoryAtDeath",
			new BehaviorCreator<VictoryAtDeathBehavior>(false)));
		behaviorNameMap_->insert(make_pair("Taunt",
			new BehaviorCreator<TauntBehavior>(false)));
		behaviorNameMap_->insert(make_pair("Say",
			new BehaviorCreator<SayBehavior>(false)));
		behaviorNameMap_->insert(make_pair("RemoveOnEmpty",
			new BehaviorCreator<RemoveOnEmptyBehavior>(false)));
		behaviorNameMap_->insert(make_pair("StayAbove",
			new BehaviorCreator<StayAboveBehavior>(true)));
		behaviorNameMap_->insert(make_pair("StayInTerrain",
			new BehaviorCreator<StayInTerrainBehavior>(true)));
		behaviorNameMap_->insert(make_pair("ReleaseSpawnAtDeath",
			new BehaviorCreator<ReleaseSpawnAtDeathBehavior>(false)));
		behaviorNameMap_->insert(make_pair("MakeMinions",
			new BehaviorCreator<MakeMinionsBehavior>(false)));
		behaviorNameMap_->insert(make_pair("Decay",
			new BehaviorCreator<DecayBehavior>(false)));
		behaviorNameMap_->insert(make_pair("OryxTaunt",
			new BehaviorCreator<OryxTauntBehavior>(false)));
		behaviorNameMap_->insert(make_pair("Orbit",
			new BehaviorCreator<OrbitBehavior>(false)));
		behaviorNameMap_->insert(make_pair("Heal",
			new BehaviorCreator<HealBehavior>(false)));
		behaviorNameMap_->insert(make_pair("StayBack",
			new BehaviorCreator<StayBackBehavior>(false)));
		behaviorNameMap_->insert(make_pair("KeepDistance",
			new BehaviorCreator<KeepDistanceBehavior>(false)));
		behaviorNameMap_->insert(make_pair("Circle",
			new BehaviorCreator<CircleBehavior>(false)));
		behaviorNameMap_->insert(make_pair("StayCloseToSpawn",
			new BehaviorCreator<StayCloseToSpawn>(false)));
		behaviorNameMap_->insert(make_pair("FollowOrders",
			new BehaviorCreator<FollowOrdersBehavior>(false)));
		behaviorNameMap_->insert(make_pair("Swirl",
			new BehaviorCreator<SwirlBehavior>(false)));
		behaviorNameMap_->insert(make_pair("Buzz",
			new BehaviorCreator<BuzzBehavior>(false)));
		behaviorNameMap_->insert(make_pair("BackAndForth",
			new BehaviorCreator<BackAndForthBehavior>(false)));
		behaviorNameMap_->insert(make_pair("Move",
			new BehaviorCreator<MoveBehavior>(false)));
		behaviorNameMap_->insert(make_pair("MaintainProtectors",
			new BehaviorCreator<MaintainProtectorsBehavior>(false)));
		behaviorNameMap_->insert(make_pair("RemainVisible",
			new BehaviorCreator<RemainVisibleBehavior>(false)));
		behaviorNameMap_->insert(make_pair("Zap",
			new BehaviorCreator<ZapBehavior>(false)));
		behaviorNameMap_->insert(make_pair("HealZap",
			new BehaviorCreator<HealZapBehavior>(false)));
		behaviorNameMap_->insert(make_pair("TrapTrigger",
			new BehaviorCreator<TrapTriggerBehavior>(false)));
		behaviorNameMap_->insert(make_pair("Shadow",
			new BehaviorCreator<ShadowBehavior>(false)));
		behaviorNameMap_->insert(make_pair("GrenadeToss",
			new BehaviorCreator<GrenadeTossBehavior>(false)));
		behaviorNameMap_->insert(make_pair("ObjectToss",
			new BehaviorCreator<ObjectTossBehavior>(false)));
		behaviorNameMap_->insert(make_pair("Walk",
			new BehaviorCreator<WalkBehavior>(false)));
		behaviorNameMap_->insert(make_pair("ShowEffect",
			new BehaviorCreator<ShowEffectBehavior>(false)));
		behaviorNameMap_->insert(make_pair("ConditionEffect",
			new BehaviorCreator<ConditionEffectBehavior>(false)));
		behaviorNameMap_->insert(make_pair("SetAltTexture",
			new BehaviorCreator<SetAltTextureBehavior>(false)));
		behaviorNameMap_->insert(make_pair("SetTag",
			new BehaviorCreator<SetTagBehavior>(false)));
		behaviorNameMap_->insert(make_pair("ClearTag",
			new BehaviorCreator<ClearTagBehavior>(false)));
		behaviorNameMap_->insert(make_pair("SetGlobalTag",
			new BehaviorCreator<SetGlobalTagBehavior>(false)));
		behaviorNameMap_->insert(make_pair("ClearGlobalTag",
			new BehaviorCreator<ClearGlobalTagBehavior>(false)));
		behaviorNameMap_->insert(make_pair("RestoresBreath",
			new BehaviorCreator<RestoresBreathBehavior>(false)));
		behaviorNameMap_->insert(make_pair("CopyDamageRecord",
			new BehaviorCreator<CopyDamageRecordBehavior>(false)));
		behaviorNameMap_->insert(make_pair("ChangeGround",
			new BehaviorCreator<ChangeGroundBehavior>(false)));
		behaviorNameMap_->insert(make_pair("PlaySound",
			new BehaviorCreator<PlaySoundBehavior>(false)));
		behaviorNameMap_->insert(make_pair("PlaceMap",
			new BehaviorCreator<PlaceMapBehavior>(false)));
		behaviorNameMap_->insert(make_pair("DeleteObject",
			new BehaviorCreator<DeleteObjectBehavior>(false)));
	}

	unordered_map<string, BehaviorCreatorBase*>::iterator iter =
		behaviorNameMap_->find(kind.name);

	if (iter == behaviorNameMap_->end()) {
		// Unrecognized name
		Util::Log() << "ERROR: Unrecognized behavior: " << kind.name << endl;
		exit(-1);
	}

	return iter->second->create(host, kind.options);
}

/* static */
double Behavior::GetFloatArgument(const XMLOptionsMap &options,
	const char* name, double defaultValue) {
	XMLOptionsMap::const_iterator it = options.find(name);
	return (it != options.end()) ? atof(it->second.c_str()) : defaultValue;
}

/* static */
int Behavior::GetIntArgument(const XMLOptionsMap &options,
	const char* name, int defaultValue) {
	XMLOptionsMap::const_iterator it = options.find(name);
	return (it != options.end()) ? atoi(it->second.c_str()) : defaultValue;
}

/* static */
uint Behavior::GetHexIntArgument(const XMLOptionsMap &options,
	const char* name, uint defaultValue) {
	XMLOptionsMap::const_iterator it = options.find(name);
	if (it == options.end()) {
		return defaultValue;
	}
	uint val;
	sscanf(it->second.c_str(), "%x", &val);
	return val;
}

/* static */
bool Behavior::GetBooleanArgument(const XMLOptionsMap &options,
	const char* name, bool defaultValue) {
	XMLOptionsMap::const_iterator it = options.find(name);
	if (it == options.end()) {
		return defaultValue;
	}
	return (it->second == "0" || it->second == "false") ? false : true;
}

/* static */
string Behavior::GetStringArgument(const XMLOptionsMap &options,
	const char* name, string defaultValue) {
	XMLOptionsMap::const_iterator it = options.find(name);
	return (it != options.end()) ? it->second : defaultValue;
}

int Behavior::findClosest(double maxDist, int type) {
	GameObject *closest = NULL;
	double closestSqDist = maxDist * maxDist;

	vector<GameObject*> objs;
	host_->game()->census()->objsWithin(host_->pos(), maxDist, &objs);
	for (uint i = 0; i < objs.size(); i++) {
		GameObject *o = objs[i];
		if (o->type() != type) {
			continue;
		}
		double sqDist = o->SqDistTo(host_);
		if (sqDist < closestSqDist) {
			closestSqDist = sqDist;
			closest = o;
		}
	}

	return closest == NULL ? -1 : closest->id();
}

WanderBehavior::WanderBehavior(GameObject *host,
	const XMLOptionsMap &options)
	: Behavior(host, options) {

	speed_ = GetFloatArgument(options, "speed", 0.4);
	avoidWater_ = GetBooleanArgument(options, "avoidWater", false);
}

bool WanderBehavior::Update(double dt) {
	host_->Wander(speed_, avoidWater_);  // Deliberately ignoring dt here
	return true;
}

ChargeBehavior::ChargeBehavior(GameObject *host,
	const XMLOptionsMap &options) : Behavior(host, options),
	cooldownLeft_(0.0), charging_(false) {

	cooldown_ = GetFloatArgument(options, "cooldown", 3.0);
	speed_ = GetFloatArgument(options, "speed", 3.0);
	range_ = GetFloatArgument(options, "range", 20.0);
}

bool ChargeBehavior::Update(double dt) {
	if (charging_) {
		Charge(dt);
		return true;
	}

	cooldownLeft_ -= dt;
	if (cooldownLeft_ > 0) {
		return false;
	}
	cooldownLeft_ = cooldown_;

	GameObject *target = host_->game()->FindPlayerTarget(host_);
	if (target == NULL || host_->DistTo(target) > range_) {
		return false;
	}

	charging_ = true;
	targetPos_ = target->pos();
	currentDist_ = targetPos_.SqDistTo(host_->pos());

	Charge(dt);

	return true;
}

void ChargeBehavior::Clear() {
	cooldownLeft_ = 0.0;
	charging_ = false;
}

void ChargeBehavior::Charge(double dt) {
	if (!host_->MoveToward(targetPos_, speed_ * dt * 5)) {
		charging_ = false;
	}
	else {
		double dist = targetPos_.SqDistTo(host_->pos());

		if (dist >= currentDist_) {
			charging_ = false;
		}
		else if (dist < .1 * .1) {
			charging_ = false;
		}

		currentDist_ = dist;
	}
}

FollowBehavior::FollowBehavior(GameObject *host,
	const XMLOptionsMap &options)
	: Behavior(host, options), following_(false), timeLeft_(0) {

	acquireRange_ = GetFloatArgument(options, "acquireRange", 10.0);
	range_ = GetFloatArgument(options, "range", 3.0);
	speed_ = GetFloatArgument(options, "speed", 1.0);
	duration_ = GetFloatArgument(options, "duration", 30.0);
	cooldown_ = GetFloatArgument(options, "cooldown", 5.0);
	predictive_ = GetBooleanArgument(options, "predictive", false);
}

bool FollowBehavior::Update(double dt) {
	timeLeft_ -= dt;
	if (timeLeft_ < 0) timeLeft_ = 0.0;
	if (!following_ && timeLeft_ > 0) return false;  // Still cooling down
	if (duration_ != 0.0 && following_ && timeLeft_ == 0.0) {
		// Give up the chase
		following_ = false;
		timeLeft_ = cooldown_;
		return false;
	}

	GameObject *target = host_->game()->FindPlayerTarget(host_);
	if (target == NULL) {
		if (following_) {
			following_ = false;
			timeLeft_ = cooldown_;
		}
		return false;
	}

	double dist = host_->DistTo(target);

	if (dist > acquireRange_) {
		if (following_) {
			// Had a target but he got away; give up the chase
			following_ = false;
			timeLeft_ = cooldown_;
		}
		return false;
	}

	if (!following_) {
		following_ = true;
		timeLeft_ = duration_;  // start the clock!
	}

	if (dist < range_) return false;

	float targetX = target->x();
	float targetY = target->y();
	if (predictive_) {
		static const int PREDICT_NUM_TICKS = 4; // magic determined by experiment
		targetX += PREDICT_NUM_TICKS * (target->x() - target->prevX());
		targetY += PREDICT_NUM_TICKS * (target->y() - target->prevY());
	}

	return host_->MoveToward(WorldPosition(targetX, targetY), speed_ * dt * 5);
}

void FollowBehavior::Clear() {
	following_ = false;
	timeLeft_ = 0;
}

ShootBehavior::ShootBehavior(GameObject *host,
	const XMLOptionsMap &options)
	: Behavior(host, options), shootType_(TARGETED), arc_(0) {

	string shootTypeStr = GetStringArgument(options, "type", "targeted");
	if (shootTypeStr == "targeted") shootType_ = TARGETED;
	else if (shootTypeStr == "auto") shootType_ = AUTO;
	range_ = GetFloatArgument(options, "range", 8.0);
	numShots_ = GetIntArgument(options, "numShots", 1);
	cooldown_ = GetFloatArgument(options, "cooldown", 1.0);
	cooldownJitter_ = GetBooleanArgument(options, "cooldownJitter", false);
	projectileId_ = GetIntArgument(options, "projectileId", 0);
	objectType_ =
		Game::ObjectTypeFromId(GetStringArgument(options, "childId", ""));
	angleInc_ = GetFloatArgument(options, "angle", 9) * TO_RADIANS;
	predictive_ = GetFloatArgument(options, "predictive", 0.0);
	defaultAngle_ = GetFloatArgument(options, "defaultAngle", 90) *
		TO_RADIANS;
	float arcDegrees = GetFloatArgument(options, "arcDegrees", 0);
	if (arcDegrees != 0) arc_ = arcDegrees / 360.0 * 2 * PI;
	blastEffect_ = GetBooleanArgument(options, "blastEffect", false);
	offset_ = GetFloatArgument(options, "offset", 0.0);

	tilShot_ = offset_;
}

void ShootBehavior::SetNewCooldown() {
	tilShot_ = MAX(0.0, tilShot_ + cooldown_);
	if (cooldownJitter_) {
		tilShot_ += Util::PlusMinus(cooldown_ / 2);
	}
}

bool ShootBehavior::Update(double dt) {
	tilShot_ -= dt;

	if (tilShot_ > 0.0) return false;

	GameObject *target = NULL;
	if (shootType_ == TARGETED) {
		if (arc_ == 0) {
			target = host_->game()->FindPlayerTarget(host_);
		}
		else {
			Angle a = host_->momentum()->facing_;
			target = host_->game()->FindPlayerTarget(host_, a.rad, arc_);
		}

		double dist = target == NULL ? 1000 : host_->DistTo(target);

		if (dist > range_ + 20) {
			SetNewCooldown();
			return false;
		}

		if (dist > range_) {
			tilShot_ = 0.0;
			return false; // no cooldown, players are pretty close
		}

		if (host_->game()->gameType() == DUNGEON_GAME &&
			!host_->game()->visibility()->CanSee(host_->ipos(), target->ipos())) {
			tilShot_ = 0.0;
			return false; // no line of sight
		}
	}

	if (host_->hasCondition(STUNNED_CONDEFFECT)) {
		tilShot_ = 0.0;
		return false; // stunned
	}

	Shoot(target);
	SetNewCooldown();
	return true;
}

void ShootBehavior::Clear() {
	tilShot_ = offset_;
}

void ShootBehavior::Shoot(GameObject* target) {
	int numShots = numShots_;
	if (host_->hasCondition(DAZED_CONDEFFECT)) {
		int newNumShots = numShots / 2;
		if (numShots % 2 == 1 && newNumShots % 2 == 0) newNumShots += 1;
		numShots = newNumShots;
	}

	float angle = defaultAngle_;
	if (target != NULL) {
		float targetX = target->x();
		float targetY = target->y();
		if (predictive_ != 0.0 && predictive_ > Util::Random(0.0, 1.0)) {
			static const int PREDICT_NUM_TICKS = 4; // magic determined by experiment
			targetX = target->x() + PREDICT_NUM_TICKS *
				(target->x() - target->prevX());
			targetY = target->y() + PREDICT_NUM_TICKS *
				(target->y() - target->prevY());
		}
		angle = atan2(targetY - host_->y(), targetX - host_->x()) -
			angleInc_ * ((numShots - 1) / 2.0);
	}
	else {
		angle = angle - angleInc_ * (numShots / 2);
	}

	WorldPosition pos(host_->x(), host_->y());

	if (objectType_ == -1) {
		const XMLProjectile* proj = host_->props()->getProjectile(projectileId_);
		if (proj == NULL) {
			Util::Log() << "ERROR: missing projectile for " << host_->idName()
				<< " id " << projectileId_ << endl;
			return;
		}
		ObjectBulletId newBullet(host_->NextEnemyBulletId(numShots), host_->id());
		Message* m = new EnemyShootMessage(
			newBullet, projectileId_, host_->type(), pos, angle,
			proj->GetDamage(), numShots, angleInc_);
		host_->game()->BroadcastIfVisible(m, host_);
		if (blastEffect_) {
			Message* m = ShowEffectMessage::ConeBlast(host_->id(), target->pos(),
				1.5, 0xffffff);
			host_->game()->BroadcastIfVisible(m, host_);
		}
	}
	else {
		for (int i = 0; i < numShots; i++) {
			host_->game()->CreateNewObject(objectType_, host_->pos());
		}
	}
}

ChangeSizeBehavior::ChangeSizeBehavior(GameObject *host,
	const XMLOptionsMap &options) : Behavior(host, options) {

	rate_ = GetFloatArgument(options, "rate", 0.2);
	maxSize_ = GetFloatArgument(options, "maxSize", 200.0);
	minSize_ = GetFloatArgument(options, "minSize", 60.0);
}

bool ChangeSizeBehavior::Update(double dt) {
	if (rate_ == 0.0 || (rate_ > 0.0 && host_->size() >= maxSize_) ||
		(rate_ < 0.0 && host_->size() <= minSize_)) {
		return false;
	}

	double newSize = host_->size() + rate_ * dt * 5;
	newSize = MAX(minSize_, MIN(maxSize_, newSize));
	host_->SetSize(newSize);
	return true;
}

DecayBehavior::DecayBehavior(GameObject *host, const XMLOptionsMap &options)
: Behavior(host, options) {
	time_ = GetFloatArgument(options, "time", 60.0);
	particleEffect_ = GetStringArgument(options, "particleEffect", "");
	color_ = GetHexIntArgument(options, "color", 0xFFFFFF);
	dropLoot_ = GetBooleanArgument(options, "dropLoot", false);
	giveXP_ = GetBooleanArgument(options, "giveXP", false);

	tilDeath_ = time_;
}

DecayBehavior::DecayBehavior(GameObject *host, double tilDeath) :
Behavior(host), time_(tilDeath), particleEffect_("Nova"),
color_(0xff0000), dropLoot_(false), giveXP_(false), tilDeath_(tilDeath) {}

bool DecayBehavior::Update(double dt) {
	tilDeath_ -= dt;
	if (tilDeath_ > 0) return false;

	if (!particleEffect_.empty()) {
		if (particleEffect_ == "Nova") {
			Message *m = ShowEffectMessage::Nova(host_->id(), 1, color_);
			host_->game()->BroadcastIfVisible(m, host_);
		}
		else {
			Util::Log() << "Unknown Particle Effect on Decay: " << particleEffect_
				<< endl;
		}
	}

	host_->Kill(NULL, "Decayed", dropLoot_, giveXP_);
	return true;
}

void DecayBehavior::Clear() {
	tilDeath_ = time_;
}

MetamorphoseBehavior::MetamorphoseBehavior(GameObject *host, const XMLOptionsMap &options)
: Behavior(host, options) {
	time_ = GetFloatArgument(options, "time", 60.0);
	childType_ =
		Game::ObjectTypeFromId(GetStringArgument(options, "childId", ""));
	dropLoot_ = GetBooleanArgument(options, "dropLoot", false);
	giveXP_ = GetBooleanArgument(options, "giveXP", false);
	if (childType_ == -1) {
		childType_ = host_->type();
	}

	tilMetamorphisis_ = time_;
}

bool MetamorphoseBehavior::Update(double dt) {
	tilMetamorphisis_ -= dt;
	if (tilMetamorphisis_ > 0) return false;

	// Find the xmlEnemy matching child type and make one
	host_->game()->CreateNewObject(childType_, host_->pos());

	host_->Kill(NULL, "Metamorphosed", dropLoot_, giveXP_);
	return true;
}

void MetamorphoseBehavior::Clear() {
	tilMetamorphisis_ = time_;
}

MetamorphoseAtDeathBehavior::MetamorphoseAtDeathBehavior(
	GameObject *host, const XMLOptionsMap &options)
	: Behavior(host, options) {
	prob_ = GetFloatArgument(options, "prob", 1.0);
	posJitter_ = GetFloatArgument(options, "posJitter", 0.1);
	childType_ =
		Game::ObjectTypeFromId(GetStringArgument(options, "childId", ""));
}

MetamorphoseAtDeathBehavior::MetamorphoseAtDeathBehavior(
	GameObject *host, const string &childId, float posJitter)
	: Behavior(host), prob_(1.0), posJitter_(posJitter) {
	childType_ = Game::ObjectTypeFromId(childId);
}

void MetamorphoseAtDeathBehavior::AtDeath(GameObject *shooter) {
	if (prob_ < 1.0 && Util::Random(0.0, 1.0) > prob_) {
		return;
	}
	WorldPosition pos = host_->pos();
	if (posJitter_ != 0.0) {
		pos = pos.PlusSalt(posJitter_);
	}
	host_->game()->CreateNewObject(childType_, pos);
}


ExplodeBehavior::ExplodeBehavior(GameObject *host, const XMLOptionsMap &options)
: Behavior(host, options) {
	initialDelay_ = GetFloatArgument(options, "initialDelay", 0.0);
	range_ = GetFloatArgument(options, "range", 5.0);
	numShots_ = GetIntArgument(options, "numShots", 12);
	arcDegrees_ = GetIntArgument(options, "arcDegrees", 360);
	projectileId_ = GetIntArgument(options, "projectileId", 0);
	dropLoot_ = GetBooleanArgument(options, "dropLoot", false);
	giveXP_ = GetBooleanArgument(options, "giveXP", false);

	tilActivate_ = initialDelay_;
}

bool ExplodeBehavior::Update(double dt) {
	tilActivate_ -= dt;
	if (tilActivate_ > 0) return false;

	double arcRadians = arcDegrees_ * TO_RADIANS;
	double angle = 0;
	if (range_ > 0.0) {
		GameObject *o = host_->game()->FindPlayerTarget(host_);
		if (!o || o->DistTo(host_) > range_) return false;
		if (host_->game()->gameType() == DUNGEON_GAME &&
			!host_->game()->visibility()->CanSee(host_->ipos(), o->ipos())) {
			return false;
		}
		angle = host_->AngleTo(o) - arcRadians / 2;
	}

	if (numShots_ > 0) {
		double angleInc = arcRadians / numShots_;
		const XMLProjectile* proj = host_->props()->getProjectile(projectileId_);
		ObjectBulletId newBullet(host_->NextEnemyBulletId(numShots_), host_->id());
		EnemyShootMessage *enemyShootMsg = new EnemyShootMessage(
			newBullet, projectileId_, host_->type(), host_->pos(),
			angle, proj->GetDamage(), numShots_, angleInc);
		host_->game()->BroadcastIfVisible(enemyShootMsg, host_);
	}

	host_->Kill(NULL, "Exploded", dropLoot_, giveXP_);

	return true;
}

void ExplodeBehavior::Clear() {
	tilActivate_ = initialDelay_;
}

ProtectBehavior::ProtectBehavior(GameObject *host,
	const XMLOptionsMap &options) : Behavior(host, options),
	reprotecting_(false), protecteeId_(-1), tilFindProtectee_(0) {

	protecteeType_ =
		Game::ObjectTypeFromId(GetStringArgument(options, "protecteeId", ""));
	if (protecteeType_ == -1) {
		Util::Log() << "Unable to find protecteeId \""
			<< GetStringArgument(options, "protecteeId", "")
			<< "\" for " << host->idName() << endl;
		exit(-1);
	}

	acquireRange_ = GetFloatArgument(options, "acquireRange", 15.0);
	protectionRange_ = GetFloatArgument(options, "protectionRange", 3.0);
	reprotectRange_ =
		GetFloatArgument(options, "reprotectRange", protectionRange_);
	speed_ = GetFloatArgument(options, "speed", 0.8);
}

bool ProtectBehavior::Update(double dt) {
	GameObject* protectee = protecteeId_ != -1 ?
		host_->game()->FindObject(protecteeId_) : NULL;


	if (protectee == NULL) {
		if (tilFindProtectee_ > 0) tilFindProtectee_ -= dt;
		if (tilFindProtectee_ <= 0) {
			protecteeId_ = FindProtectee();
			if (protecteeId_ == -1) tilFindProtectee_ = 20.0;
		}
		return false;
	}

	double dist = protectee->DistTo(host_);

	// If outside protectionRange, go into reprotect mode
	if (dist > protectionRange_) reprotecting_ = true;

	// If reprotecting, move toward the protectee
	if (!reprotecting_) return false;

	if (dist < reprotectRange_) {
		reprotecting_ = false;
		return false;
	}

	return host_->MoveToward(protectee->pos(), speed_ * dt * 5);
}

void ProtectBehavior::Clear() {
	tilFindProtectee_ = 0.0;
	reprotecting_ = false;
}

int ProtectBehavior::FindProtectee() {
	GameObject *closest = NULL;
	double closestDist = acquireRange_;

	vector<GameObject*> objs;
	host_->game()->census()->objsWithin(host_->pos(), acquireRange_, &objs);
	for (uint i = 0; i < objs.size(); i++) {
		GameObject *o = objs[i];
		if (o->type() != protecteeType_) continue;
		double dist = o->DistTo(host_);
		if (dist < closestDist) {
			closestDist = dist;
			closest = o;
		}
	}

	return closest == NULL ? -1 : closest->id();
}

VictoryAtDeathBehavior::VictoryAtDeathBehavior(GameObject *host,
	const XMLOptionsMap &options) : Behavior(host, options) {}

void VictoryAtDeathBehavior::AtDeath(GameObject *shooter) {
	string portalId;
	if (host_->type() == Game::ObjectTypeFromId("Oryx the Mad God 1")) {
		portalId = "Locked Wine Cellar Portal";
	}
	else {
		portalId = "Portal to Nexus";
	}

	// Make portal slightly off corpse to avoid conflict with loot bag
	host_->game()->CreateNewObject(portalId, host_->pos() + .2);

	App()->stats()->Increment("oryx_kills");
}



TauntBehavior::TauntBehavior(GameObject *host, const XMLOptionsMap &options)
: Behavior(host, options), sinceTaunt_(300.0) {

	list_ = GetStringArgument(options, "listId", host->idName());
	broadcast_ = GetBooleanArgument(options, "broadcast", false);
}

bool TauntBehavior::Update(double dt) {
	sinceTaunt_ += dt;
	if (sinceTaunt_ < 20) return false;
	sinceTaunt_ = 0;

	return DoTaunt("everySoOften");
}

void TauntBehavior::AtDeath(GameObject *shooter) {
	DoTaunt("mydeath");
}

bool TauntBehavior::DoTaunt(const string &list) {
	GameObject *target = host_->game()->FindClosestPlayer(host_, true);
	if (!broadcast_ && target == NULL) return false;

	params_["HITPOINTS"] = Util::ToString(host_->hitpoints());
	params_["PLAYER"] = target ? target->name() : "Rob";

	string taunt = StringLists::GetString(list_, list, params_);

	if (taunt.size() == 0) return false;

	string chatName = "#" + host_->props()->displayId;
	TextMessage* m = new TextMessage(chatName.c_str(), host_->id(),
		-1, 10, "", taunt.c_str(), "");
	if (broadcast_) {
		host_->game()->Broadcast(m);
	}
	else {
		host_->game()->BroadcastIfVisible(m, host_);
	}

	return true;
}

SayBehavior::SayBehavior(GameObject *host, const XMLOptionsMap &options)
: Behavior(host, options), done_(false) {

	string textList = GetStringArgument(options, "text", "");
	Util::Tokenize(textList, ";", &texts_);
	broadcast_ = GetBooleanArgument(options, "broadcast", false);
	prob_ = GetFloatArgument(options, "prob", 1.0);
	bubbleTime_ = GetIntArgument(options, "bubbleTime", 10);
}

bool SayBehavior::Update(double dt) {
	if (done_) {
		return false;
	}

	if (prob_ != 1.0 && Util::Random(0.0, 1.0) > prob_) {
		done_ = true;
		return false;
	}

	string chatName = "#" + host_->props()->displayId;
	const string& text = texts_[Util::Random(0, texts_.size() - 1)];
	TextMessage* m = new TextMessage(chatName.c_str(), host_->id(),
		-1, bubbleTime_, "", text.c_str(), "");
	if (broadcast_) {
		host_->game()->Broadcast(m);
	}
	else {
		host_->game()->BroadcastIfVisible(m, host_);
	}

	done_ = true;
	return true;
}

void SayBehavior::Clear() {
	done_ = false;
}

RemoveOnEmptyBehavior::RemoveOnEmptyBehavior(GameObject *host,
	const XMLOptionsMap &options) : Behavior(host, options) {}

bool RemoveOnEmptyBehavior::Update(double dt) {
	if (!host_->InventoryEmpty()) return false;
	host_->Kill(NULL, "Became Empty", false, false);
	return true;
}

StayAboveBehavior::StayAboveBehavior(GameObject *host,
	const XMLOptionsMap &options) : Behavior(host, options),
	initialLookRadius_(5), lookIncrement_(3), tilLook_(0), engage_(false) {

	minAltitude_ = GetIntArgument(options, "minAltitude", 200);
	speed_ = GetFloatArgument(options, "speed", 0.5);

	lookRadius_ = initialLookRadius_;
}

bool StayAboveBehavior::Update(double dt) {
	tilLook_ -= dt;
	if (!engage_ && tilLook_ > 0) {
		return false;
	}

	if (tilLook_ <= 0 || dest_ == host_->ipos()) {
		tilLook_ = 20.0;

		int alt = host_->game()->Map(host_->pos()).z;
		if (alt >= minAltitude_) {
			lookRadius_ = initialLookRadius_;  // ahh home again, hope I never leave
			engage_ = false;
			return false;
		}

		engage_ = true;
		dest_ = FindHighestPoint(lookRadius_, 1);
		if (dest_ == host_->ipos()) {
			lookRadius_ += lookIncrement_; // At local max.  Look wider next time.
			tilLook_ = 0;
			return true;
		}
	}

	if (!engage_) return false;

	return host_->MoveToward(dest_, speed_ * dt * 5);
}

IntPoint StayAboveBehavior::FindHighestPoint(int radius, int skip) {
	IntPoint pos(host_->x(), host_->y());
	int highest = host_->game()->Map(host_->pos()).z;
	IntPoint highestPos(pos);

	int worldWidth = host_->game()->worldWidth();
	for (int i = MAX(0, pos.x - radius);
		i <= MIN(worldWidth - 1, pos.x + radius); i += skip) {
		for (int j = MAX(0, pos.y - radius);
			j <= MIN(worldWidth - 1, pos.y + radius); j += skip) {
			const Tile &t = host_->game()->Map(i, j);
			if (t.z > highest) {
				highest = t.z;
				highestPos = IntPoint(i, j);
			}
		}
	}

	return highestPos;
}

StayInTerrainBehavior::StayInTerrainBehavior(GameObject *host,
	const XMLOptionsMap &options) : Behavior(host, options),
	initialLookRadius_(5), lookIncrement_(3), tilLook_(0), engage_(false) {

	string terrainName = GetStringArgument(options, "terrain", "");
	if (terrainName.empty()) {
		terrain_ = host->enemy() != NULL ? host->enemy()->terrain : UNKNOWN_TERRAIN;
	}
	else {
		terrain_ = Terrain::nameToTerrain(terrainName.c_str());
	}
	minAltitude_ = GetIntArgument(options, "minAltitude",
		Terrain::getMinAltitude(terrain_));
	maxAltitude_ = GetIntArgument(options, "maxAltitude",
		Terrain::getMaxAltitude(terrain_));
	speed_ = GetFloatArgument(options, "speed", 0.5);

	lookRadius_ = initialLookRadius_;
}

bool StayInTerrainBehavior::Update(double dt) {
	tilLook_ -= dt;
	if (!engage_ && tilLook_ > 0) {
		return false;
	}

	if (tilLook_ <= 0) {
		tilLook_ = 20.0;

		const Tile& t = host_->game()->Map(host_->pos());
		if (ScoreTile(t) == 0) {
			lookRadius_ = initialLookRadius_;  // ahh home again, hope I never leave
			engage_ = false;
			return false;
		}

		engage_ = true;
		dest_ = FindBestPoint(lookRadius_);
	}

	if (!engage_) return false;

	IntPoint pos(host_->x(), host_->y());
	if (dest_ == pos) {
		lookRadius_ += lookIncrement_; // At local max.  Look wider next time.
		tilLook_ = 0;
		return false;
	}

	return host_->MoveToward(dest_, speed_ * dt * 5);
}

int StayInTerrainBehavior::ScoreTile(const Tile& t) {
	int score = 0;
	if (terrain_ != UNKNOWN_TERRAIN &&
		(t.terrain & terrain_) != terrain_) score += 0xFF;
	if (t.z < minAltitude_) score += minAltitude_ - t.z;
	else if (t.z > maxAltitude_) score += t.z - maxAltitude_;
	return score;
}

IntPoint StayInTerrainBehavior::FindBestPoint(int radius) {
	IntPoint pos(host_->x(), host_->y());
	const Tile& t = host_->game()->Map(host_->pos());
	int bestScore = ScoreTile(t);
	IntPoint bestPos(pos);

	int worldWidth = host_->game()->worldWidth();
	for (int i = MAX(0, pos.x - radius);
		i <= MIN(worldWidth - 1, pos.x + radius); i++) {
		for (int j = MAX(0, pos.y - radius);
			j <= MIN(worldWidth - 1, pos.y + radius); j++) {
			const Tile &t = host_->game()->Map(i, j);
			int score = ScoreTile(t);
			if (score < bestScore) {
				bestScore = score;
				bestPos = IntPoint(i, j);
			}
		}
	}

	return bestPos;
}

ReleaseSpawnAtDeathBehavior::ReleaseSpawnAtDeathBehavior(GameObject *host,
	const XMLOptionsMap &options) : Behavior(host, options) {

	min_ = GetIntArgument(options, "min", 2);
	max_ = GetIntArgument(options, "max", 7);
	childType_ =
		Game::ObjectTypeFromId(GetStringArgument(options, "childId", ""));
	if (childType_ == -1) {
		childType_ = host_->type();
	}
}

void ReleaseSpawnAtDeathBehavior::AtDeath(GameObject *shooter) {
	int num = Util::Random(min_, max_);

	XMLEnemy *e = Game::XmlEnemyFromType(childType_);
	if (e == NULL) return;
	for (int i = 0; i < num; i++) {
		host_->game()->AddObject(e->MakeOne(host_->game(), host_->pos()));
	}
}


MakeMinionsBehavior::MakeMinionsBehavior(GameObject *host,
	const XMLOptionsMap &options) : Behavior(host, options),
	cooldownLeft_(0.0) {

	cooldown_ = GetFloatArgument(options, "cooldown", 30.0);
	cooldownJitter_ = GetBooleanArgument(options, "cooldownJitter", false);

	XMLOptionsMap::const_iterator it = options.find("childIds");
	if (it != options.end()) {
		vector<string> childIds;
		Util::Tokenize(it->second, ",", &childIds);
		for (uint i = 0; i < childIds.size(); i++) {
			childTypes_.push_back(Game::ObjectTypeFromId(childIds[i]));
		}
	}

	it = options.find("groupId");  // override childId
	if (it != options.end()) {
		const vector<int>* group = Game::EnemyTypesFromGroup(it->second);
		childTypes_.insert(childTypes_.end(), group->begin(), group->end());
	}
	if (childTypes_.empty()) {
		childTypes_.push_back(host->type());
	}

	maxChildren_ = GetIntArgument(options, "maxChildren", 0);
	minOffset_ = GetFloatArgument(options, "minOffset", 0.0);
	maxOffset_ = GetFloatArgument(options, "maxOffset", 0.0);
	bool startWithMinions = GetBooleanArgument(options, "startWithMinions", true);

	if (startWithMinions) {
		for (uint i = 0; i < maxChildren_; i++) {
			if (!MakeChild()) return;
		}
	}
}

bool MakeMinionsBehavior::Update(double dt) {
	cooldownLeft_ -= dt;
	if (cooldownLeft_ > 0) return false;

	cooldownLeft_ = cooldown_;
	if (cooldownJitter_) {
		cooldownLeft_ += Util::PlusMinus(cooldown_ / 2);
	}

	if (maxChildren_ > 0) {
		uint i = 0;
		while (i < children_.size()) {
			int id = children_[i];
			if (host_->game()->IsAlive(id)) {
				i++;
				continue;
			}
			children_[i] = children_.back();
			children_.pop_back();
		}

		if (children_.size() >= maxChildren_) {
			return false;
		}
	}

	return MakeChild();
}

void MakeMinionsBehavior::Clear() {
	cooldownLeft_ = 0.0;
}

bool MakeMinionsBehavior::MakeChild() {
	int childType = childTypes_[Util::Random(0, childTypes_.size() - 1)];
	WorldPosition pos = host_->pos();
	if (maxOffset_ > 0.0) {
		float d = Util::Random(minOffset_, maxOffset_);
		float a = Util::Random(0.0, 2 * PI);
		pos.x += d * cos(a);
		pos.y += d * sin(a);
	}
	GameObject* go = host_->game()->CreateNewObject(childType, pos);
	if (go == NULL) return false;

	if (maxChildren_ > 0) {
		children_.push_back(go->id());
	}
	return true;
}


OrbitBehavior::OrbitBehavior(GameObject *host,
	const XMLOptionsMap &options) : Behavior(host, options), protecteeId_(-1),
	tilFindProtectee_(0), phasePosition_(0) {

	protecteeType_ =
		Game::ObjectTypeFromId(GetStringArgument(options, "protecteeId", ""));
	if (protecteeType_ == -1) {
		Util::Log() << "Unable to find Orbit protecteeId \""
			<< GetStringArgument(options, "protecteeId", "")
			<< "\" for " << host->idName() << endl;
		exit(-1);
	}

	direction_ = GetBooleanArgument(options, "clockwise", false) ? -1 : 1;
	acquireRange_ = GetFloatArgument(options, "acquireRange", 15.0);
	speed_ = GetFloatArgument(options, "speed", 0.8);
	radius_ = GetFloatArgument(options, "radius", 5.0);
	speedVariability_ = GetFloatArgument(options, "speedVariability", 0.2);
	radiusVariability_ = GetFloatArgument(options, "radiusVariability", 0.5);
	radiusWobbleDistance_ =
		GetFloatArgument(options, "radiusWobbleDistance", 0.0);
	radiusWobblePeriod_ = GetFloatArgument(options, "radiusWobblePeriod", 15.0);

	speed_ += Util::PlusMinus(speed_ * speedVariability_);
	radius_ += Util::PlusMinus(radius_ * radiusVariability_);
}

bool OrbitBehavior::Update(double dt) {
	GameObject* protectee = protecteeId_ != -1 ?
		host_->game()->FindObject(protecteeId_) : NULL;

	if (protectee == NULL) {
		if (tilFindProtectee_ > 0) tilFindProtectee_ -= dt;
		if (tilFindProtectee_ <= 0) {
			protecteeId_ = FindProtectee();
			if (protecteeId_ == -1) tilFindProtectee_ = 20.0;
		}
		// Nothing to orbit this tick
		return false;
	}

	double currentWobble = 0;
	if (radiusWobbleDistance_ > 0) {
		phasePosition_ += dt / radiusWobblePeriod_;
		while (phasePosition_ > 1.0) phasePosition_ -= 1.0;
		currentWobble = sin(phasePosition_ * 2 * PI) * radiusWobbleDistance_;
	}

	// Find angle to desired point
	double angle =
		protectee->AngleTo(host_) - direction_ * speed_ * dt * 5 / radius_;

	// Find point on circle
	WorldPosition p = protectee->pos().PointAt(angle, radius_ + currentWobble);

	// Goto that point
	bool moved = host_->MoveToward(p, speed_ * dt * 5);

	if (!moved) {
		direction_ = -direction_;
		return false;
	}

	return true;
}

int OrbitBehavior::FindProtectee() {
	GameObject *closest = NULL;
	double closestDist = acquireRange_;

	vector<GameObject*> objs;
	host_->game()->census()->objsWithin(host_->pos(), acquireRange_, &objs);
	for (uint i = 0; i < objs.size(); i++) {
		GameObject *o = objs[i];
		if (o->type() != protecteeType_) continue;
		double dist = o->DistTo(host_);
		if (dist < closestDist) {
			closestDist = dist;
			closest = o;
		}
	}

	return closest == NULL ? -1 : closest->id();
}

OryxTauntBehavior::OryxTauntBehavior(GameObject *host,
	const XMLOptionsMap &options) : Behavior(host, options) {

	string taunt = StringLists::GetString(host->idName(), "new", "");
	host_->game()->Broadcast(new TextMessage(ORYX_CHAT_NAME, taunt));
}

void OryxTauntBehavior::AtDeath(GameObject *shooter) {
	string taunt;
	if (shooter && shooter->isPlayer()) {
		taunt = StringLists::GetString(host_->idName(), "killed", shooter->name());
	}
	else {
		taunt = StringLists::GetString(host_->idName(), "death", "");
	}
	host_->game()->Broadcast(new TextMessage(ORYX_CHAT_NAME, taunt));
}

HealBehavior::HealBehavior(GameObject *host,
	const XMLOptionsMap &options) : Behavior(host, options),
	findTargetsLeft_(0) {

	XMLOptionsMap::const_iterator it;
	it = options.find("targetIds");
	if (it != options.end()) {
		vector<string> targetIds;
		Util::Tokenize(it->second, ",", &targetIds);
		for (uint i = 0; i < targetIds.size(); i++) {
			healTargetIds_.insert(Game::ObjectTypeFromId(targetIds[i]));
		}
	}

	it = options.find("targetGroups");
	vector<string> targetGroups;
	if (it != options.end()) {
		Util::Tokenize(it->second, ",", &targetGroups);
	}
	else if (healTargetIds_.size() == 0) {
		targetGroups.push_back(host->enemy()->group);
	}
	for (uint i = 0; i < targetGroups.size(); i++) {
		const vector<int>* enemyIds = Game::EnemyTypesFromGroup(targetGroups[i]);
		healTargetIds_.insert(enemyIds->begin(), enemyIds->end());
	}

	cooldown_ = GetFloatArgument(options, "cooldown", 10.0);
	range_ = GetFloatArgument(options, "range", 10.0);
	maxTargets_ = GetIntArgument(options, "maxTargets", -1);
	maxHeal_ = GetIntArgument(options, "maxHeal", -1);
	hideVisual_ = GetBooleanArgument(options, "hideVisual", false);

	cooldownLeft_ = cooldown_;
}

bool HealBehavior::Update(double dt) {
	cooldownLeft_ -= dt;
	findTargetsLeft_ -= dt;

	if (cooldownLeft_ > 0) {
		return false;
	}

	if (findTargetsLeft_ <= 0) {
		FindHealTargets();
		findTargetsLeft_ = 5.0;
	}

	vector<GameObject*> needHeals_;
	if (!HealsNeeded(&needHeals_)) {
		cooldownLeft_ = 1.0;
		return false;
	}

	bool showVisual = !hideVisual_;

	for (uint i = 0; i < needHeals_.size(); i++) {
		GameObject* go = needHeals_[i];

		if (showVisual) {
			Message *m;
			m = ShowEffectMessage::Line(go->id(), host_->pos(), 0xFFFFFF);
			host_->game()->BroadcastIfVisible(m, go);
		}

		go->Heal(maxHeal_ < 0 ? go->maxHp() : maxHeal_, hideVisual_);
	}
	cooldownLeft_ = cooldown_;
	return true;
}

bool HealBehavior::HealsNeeded(vector<GameObject*>* needHeals) {
	for (uint i = 0; i < healTargets_.size(); i++) {
		int targetId = healTargets_[i];
		GameObject* go = host_->game()->FindObject(targetId);
		if (go != NULL && go->hitpoints() < go->maxHp() &&
			host_->SqDistTo(go) < range_ * range_) {
			needHeals->push_back(go);
		}
	}
	if (maxTargets_ >= 0 && int(needHeals->size()) > maxTargets_) {
		needHeals->resize(maxTargets_);
	}
	return (needHeals->size() > 0);
}

void HealBehavior::FindHealTargets() {
	healTargets_.clear();

	vector<GameObject*> objs;
	host_->game()->census()->objsWithin(host_->pos(), range_, &objs);
	for (uint i = 0; i < objs.size(); i++) {
		GameObject *go = objs[i];
		if (healTargetIds_.find(go->type()) == healTargetIds_.end()) {
			continue;
		}
		healTargets_.push_back(go->id());
	}
}

StayBackBehavior::StayBackBehavior(GameObject *host,
	const XMLOptionsMap &options) : Behavior(host, options) {
	distance_ = GetFloatArgument(options, "distance", 10.0);
	speed_ = GetFloatArgument(options, "speed", 1.0);
}

bool StayBackBehavior::Update(double dt) {
	GameObject *target = host_->game()->FindPlayerTarget(host_);

	if (target == NULL || host_->DistTo(target) > distance_) {
		return false;
	}

	double tx = 2 * host_->pos().x - target->pos().x;
	double ty = 2 * host_->pos().y - target->pos().y;

	return host_->MoveToward(WorldPosition(tx, ty), speed_ * dt * 5);
}

KeepDistanceBehavior::KeepDistanceBehavior(GameObject *host,
	const XMLOptionsMap &options) : Behavior(host, options) {
	acquireRange_ = GetFloatArgument(options, "acquireRange", 15.0);
	distance_ = GetFloatArgument(options, "distance", 10.0);
	speed_ = GetFloatArgument(options, "speed", 1.0);
}

bool KeepDistanceBehavior::Update(double dt) {
	GameObject *target = host_->game()->FindPlayerTarget(host_);

	if (target == NULL) {
		return false;
	}

	double dist = host_->DistTo(target);
	if (dist > acquireRange_ || fabs(dist - distance_) < 1.0) {
		return false;
	}

	double tx, ty;
	if (dist > distance_) {
		tx = target->x();
		ty = target->y();
	}
	else {
		tx = 2 * host_->pos().x - target->pos().x;
		ty = 2 * host_->pos().y - target->pos().y;
	}

	return host_->MoveToward(WorldPosition(tx, ty), speed_ * dt * 5);
}

CircleBehavior::CircleBehavior(GameObject *host,
	const XMLOptionsMap &options) : Behavior(host, options) {
	acquireRange_ = GetFloatArgument(options, "acquireRange", 15.0);
	distance_ = GetFloatArgument(options, "distance", 8.0);
	speed_ = GetFloatArgument(options, "speed", 1.0);
	circleRight_ = Util::Random(0, 1) == 0;
}

bool CircleBehavior::Update(double dt) {
	GameObject *target = host_->game()->FindPlayerTarget(host_);

	if (target == NULL || host_->DistTo(target) > acquireRange_) {
		return false;
	}

	double dx = host_->pos().x - target->pos().x;
	double dy = host_->pos().y - target->pos().y;

	double angle = atan2(dy, dx) +
		(circleRight_ ? 1 : -1) * speed_ * dt * 5 / distance_;
	double newX = target->pos().x + cos(angle) * distance_;
	double newY = target->pos().y + sin(angle) * distance_;

	return host_->MoveToward(WorldPosition(newX, newY), speed_ * dt * 5);
}

StayCloseToSpawn::StayCloseToSpawn(GameObject *host,
	const XMLOptionsMap &options) : Behavior(host, options) {
	pos_ = host->pos();
	speed_ = GetFloatArgument(options, "speed", 1.0);
	range_ = GetFloatArgument(options, "range", 7.0);
	cooldown_ = GetFloatArgument(options, "cooldown", 3.0);

	engaged_ = false;
	cooldownLeft_ = 0;
}

bool StayCloseToSpawn::Update(double dt) {
	cooldownLeft_ -= dt;

	if (!engaged_ && cooldownLeft_ > 0) {
		return false;
	}

	bool closeEnough = pos_.SqDistTo(host_->pos()) < range_ * range_;

	if (closeEnough) {
		engaged_ = false;
		cooldownLeft_ = cooldown_;
		return false;
	}

	engaged_ = true;
	host_->MoveToward(pos_, speed_ * dt * 5);

	return true;
}

FollowOrdersBehavior::FollowOrdersBehavior(GameObject *host,
	const XMLOptionsMap &options) : Behavior(host, options),
	listenObjectId_(-1), cooldownLeft_(0), orders_(NULL), state_(NULL) {

	XMLOptionsMap::const_iterator it;

	it = options.find("listenIds");
	if (it != options.end()) {
		vector<string> listenIds;
		Util::Tokenize(it->second, ",", &listenIds);
		for (uint i = 0; i < listenIds.size(); i++) {
			listenTypes_.insert(Game::ObjectTypeFromId(listenIds[i]));
		}
	}

	acquireRange_ = GetFloatArgument(options, "acquireRange", 15.0);
	cooldown_ = GetFloatArgument(options, "cooldown", 3.0);
}

FollowOrdersBehavior::~FollowOrdersBehavior() {
	delete state_;
}

void FollowOrdersBehavior::Clear() {
	listenObjectId_ = -1;
	cooldownLeft_ = 0.0;
	delete state_;
	state_ = NULL;
	orders_ = NULL;
}

bool FollowOrdersBehavior::Update(double dt) {
	cooldownLeft_ = MAX(0.0, cooldownLeft_ - dt);

	if (listenObjectId_ == -1) {
		if (cooldownLeft_ > 0.0) {
			return false;
		}

		listenObjectId_ = FindClosestListen();
		cooldownLeft_ = cooldown_;
	}

	GameObject* go = host_->game()->FindObject(listenObjectId_);
	if (go == NULL || go->dead()) {
		listenObjectId_ = -1;
		return false;
	}

	const XMLState* newOrders = go->GetOrders(host_->idName());
	if (newOrders != orders_) {
		orders_ = newOrders;
		delete state_;
		state_ = (orders_ != NULL) ? orders_->NewState(host_, true) : NULL;
	}

	if (state_ == NULL) {
		return false;
	}

	state_->Update(dt);
	return true;
}

int FollowOrdersBehavior::FindClosestListen() {
	GameObject *closest = NULL;
	double closestDist = acquireRange_;

	vector<GameObject*> objs;
	host_->game()->census()->objsWithin(host_->pos(), acquireRange_, &objs);
	for (uint i = 0; i < objs.size(); i++) {
		GameObject *o = objs[i];
		if (listenTypes_.find(o->type()) == listenTypes_.end()) continue;
		double dist = o->DistTo(host_);
		if (dist < closestDist) {
			closestDist = dist;
			closest = o;
		}
	}

	return closest == NULL ? -1 : closest->id();
}

SwirlBehavior::SwirlBehavior(GameObject *host,
	const XMLOptionsMap &options) : Behavior(host, options) {
	speed_ = GetFloatArgument(options, "speed", 2.0);
	acceleration_ = GetFloatArgument(options, "acceleration", 2.0);
	turnRate_ = GetFloatArgument(options, "turnRate", PI);
}

bool SwirlBehavior::Update(double dt) {
	Momentum *m = host_->momentum();
	m->facing_ = m->facing_ + turnRate_ * dt;

	if (m->speed_ < speed_) {
		m->speed_ += acceleration_ * dt;
		if (m->speed_ > speed_) m->speed_ = speed_;
	}
	else if (m->speed_ > speed_) {
		m->speed_ -= acceleration_ * dt;
		if (m->speed_ < speed_) m->speed_ = speed_;
	}

	return m->Move(dt);
}

BuzzBehavior::BuzzBehavior(GameObject *host,
	const XMLOptionsMap &options) : Behavior(host, options),
	tilFindTarget_(0), hasTarget_(false) {
	speed_ = GetFloatArgument(options, "speed", 15.0);
	acceleration_ = GetFloatArgument(options, "acceleration", 5.0);
	turnRate_ = GetFloatArgument(options, "turnRate", PI);
	acquireRange_ = GetFloatArgument(options, "acquireRange", 20.0);
	cooldown_ = GetFloatArgument(options, "cooldown", 2.0);
}

bool BuzzBehavior::Update(double dt) {
	if (!hasTarget_) {
		tilFindTarget_ -= dt;
		if (tilFindTarget_ > 0) return false;

		// Find a target
		GameObject *o = host_->game()->FindPlayerTarget(host_);
		if (o == NULL || host_->DistTo(o) > acquireRange_) {
			tilFindTarget_ = cooldown_;
			return false;
		}

		// Yay found a target
		target_ = o->pos();
		hasTarget_ = true;
		locked_ = false;
		buzzed_ = false;
	}

	Momentum *m = host_->momentum();

	if (!locked_) {
		m->speed_ -= acceleration_ * dt / 4;  // Slow down until we get locked
		if (m->speed_ < 0) m->speed_ = 0;
	}
	else {
		// Accelerate to ramming speed
		if (m->speed_ < speed_) {
			m->speed_ += acceleration_ * dt;
			if (m->speed_ > speed_) m->speed_ = speed_;
		}
		else if (m->speed_ < speed_) {    // BUGBUG THIS IS A BUG < or > ?
			m->speed_ -= acceleration_ * dt;
			if (m->speed_ < speed_) m->speed_ = speed_;
		}
	}

	// Find arc to target
	double arc = m->facing_.ArcTo(host_->AngleTo(target_));

	if (locked_ && fabs(arc) > PI / 2) buzzed_ = true; // target behind us

	if (!locked_) {
		// Adjust facing
		double maxTurn = turnRate_ * dt;
		if (fabs(arc) > maxTurn) {
			arc = (arc < 0) ? -maxTurn : maxTurn;
		}
		if (fabs(arc) < PI / 50 ||
			host_->pos().SqDistTo(target_) < 1) locked_ = true; // good enough; lock
		else m->facing_ = m->facing_ + arc;                     // keep steering
	}


	if (!m->Move(dt)) return false;

	const double BUZZRANGE = acquireRange_ / 3;
	if (buzzed_ && host_->pos().SqDistTo(target_) > BUZZRANGE * BUZZRANGE) {
		// Buzz run is done
		hasTarget_ = false;
		tilFindTarget_ = cooldown_;
	}

	return true;
}

void BuzzBehavior::Clear() {
	tilFindTarget_ = 0.0;
	hasTarget_ = false;
}

BackAndForthBehavior::BackAndForthBehavior(GameObject *host,
	const XMLOptionsMap &options) : Behavior(host, options),
	hasCenter_(false), moveInward_(false), direction_(false) {
	speed_ = GetFloatArgument(options, "speed", 1.0);
	angle_ = GetFloatArgument(options, "angle", 0.0);
	turnRate_ = GetFloatArgument(options, "turnRate", 0.0);
	radius_ = GetFloatArgument(options, "radius", 4.0);
}

bool BackAndForthBehavior::Update(double dt) {
	if (!hasCenter_) {
		hasCenter_ = true;
		center_ = host_->pos();
	}

	angle_ = angle_ + turnRate_ * dt;
	WorldPosition target(
		center_.x + cos(angle_) * radius_,
		center_.y + sin(angle_) * radius_);
	if (!host_->MoveToward(target, speed_ * dt * 5, false) ||
		host_->pos() == target) {
		angle_ += PI;
	}

	return true;
}

void BackAndForthBehavior::Clear() {
	hasCenter_ = false;
}

MoveBehavior::MoveBehavior(GameObject *host,
	const XMLOptionsMap &options) : Behavior(host, options),
	cooldownLeft_(0.0) {
	speed_ = GetFloatArgument(options, "speed", 1.0);
	distance_ = GetFloatArgument(options, "distance", 3.0);
	angle_ = GetFloatArgument(options, "angle", 0.0) * TO_RADIANS;
	cooldown_ = GetFloatArgument(options, "cooldown", 10.0);
}

MoveBehavior::MoveBehavior(GameObject *host, double speed,
	double distance, double angle, double cooldown) : Behavior(host),
	speed_(speed), distance_(distance), angle_(angle), cooldown_(cooldown),
	cooldownLeft_(0.0) {}

bool MoveBehavior::Update(double dt) {
	if (cooldownLeft_ <= 0) {
		target_.x = host_->x() + cos(angle_) * distance_;
		target_.y = host_->y() + sin(angle_) * distance_;
		cooldownLeft_ = cooldown_;
	}
	cooldownLeft_ -= dt;

	if (host_->pos() == target_) {
		return false;
	}

	if (!host_->MoveToward(target_, speed_ * dt * 5, false)) {
		target_ = host_->pos();
		return false;
	}

	return true;
}

void MoveBehavior::Clear() {
	cooldownLeft_ = 0;
}

MaintainProtectorsBehavior::MaintainProtectorsBehavior(GameObject *host,
	const XMLOptionsMap &options) : Behavior(host, options),
	coolingDown_(false) {

	cooldown_ = GetIntArgument(options, "cooldown", 30);

	XMLOptionsMap::const_iterator it = options.find("childIds");
	if (it != options.end()) {
		vector<string> childIds;
		Util::Tokenize(it->second, ",", &childIds);
		for (uint i = 0; i < childIds.size(); i++) {
			childTypes_.push_back(Game::ObjectTypeFromId(childIds[i]));
		}
	}

	childCorpseId_ = GetStringArgument(options, "childCorpseId", "");

	it = options.find("groupId");  // override childId
	if (it != options.end()) {
		const vector<int>* group = Game::EnemyTypesFromGroup(it->second);
		childTypes_.insert(childTypes_.end(), group->begin(), group->end());
	}

	if (childTypes_.empty()) {
		childTypes_.push_back(host->type());
	}

	numChildren_ = GetIntArgument(options, "numChildren", 2);

	distance_ = GetIntArgument(options, "distance", 0);

	cooldownLeft_ = cooldown_ + Util::PlusMinus(cooldown_ / 2);
	children_.resize(numChildren_, -1);
	records_.resize(numChildren_);

	MakeChildren();
}

bool MaintainProtectorsBehavior::Update(double dt) {
	// Send stream effect
	for (uint i = 0; i < children_.size(); i++) {
		GameObject* obj = host_->game()->FindObject(children_[i]);
		if (obj == NULL || obj->dead()) {
			continue;
		}
		if (obj->damageRecord()->total() != records_[i].total()) {
			records_[i].copy(*obj->damageRecord());
		}
		uint j = rand() % (numChildren_ - 1);
		if (j >= i) {
			j++;
		}
		Message *m = ShowEffectMessage::Stream(ChildPos(i), ChildPos(j),
			coolingDown_ ? 0xFFFF00 : 0xFF0000);
		host_->game()->BroadcastIfVisible(m, host_);
	}

	if (!coolingDown_) {
		// Check for any dead children
		for (uint i = 0; i < children_.size(); i++) {
			if (host_->game()->IsAlive(children_[i])) {
				continue;
			}

			coolingDown_ = true;
			cooldownLeft_ = cooldown_;
			break;
		}

		return false;
	}

	// Check for all children dead
	bool allProtectorsDead = true;
	for (uint i = 0; i < children_.size(); i++) {
		if (host_->game()->IsAlive(children_[i])) {
			allProtectorsDead = false;
			break;
		}
	}
	if (allProtectorsDead) {
		// Create corpse children
		for (uint i = 0; i < children_.size(); i++) {
			GameObject* obj =
				host_->game()->CreateNewObject(childCorpseId_, ChildPos(i));
			if (obj == NULL) {
				continue;
			}
			obj->SetDamageRecord(records_[i]);
		}

		host_->Kill(NULL, "All Protectors Dead", true, true);
		return false;
	}

	cooldownLeft_ -= dt;
	if (cooldownLeft_ > 0) return false;

	MakeChildren();
	coolingDown_ = false;

	return true;
}

WorldPosition MaintainProtectorsBehavior::ChildPos(int i) {
	double angle = 2 * PI / numChildren_ * i;
	return host_->pos().PointAt(angle, distance_);
}

void MaintainProtectorsBehavior::MakeChildren() {
	for (uint i = 0; i < children_.size(); i++) {
		if (host_->game()->IsAlive(children_[i])) continue;

		int childType = childTypes_[Util::Random(0, childTypes_.size() - 1)];
		GameObject* go = host_->game()->CreateNewObject(childType, ChildPos(i));
		if (go == NULL) {
			continue;
		}

		children_[i] = go->id();
	}
}



RemainVisibleBehavior::RemainVisibleBehavior(GameObject *host,
	const XMLOptionsMap &options) : Behavior(host, options) {

	unordered_set<GameObject *> *alwaysVisible = host_->game()->alwaysVisible();
	alwaysVisible->insert(host_);
}

void RemainVisibleBehavior::AtDeath(GameObject *shooter) {
	unordered_set<GameObject *> *alwaysVisible = host_->game()->alwaysVisible();
	alwaysVisible->erase(host_);
}



PoisonGrenadeBehavior::PoisonGrenadeBehavior(GameObject *host,
	const XMLOptionsMap &options) : Behavior(host, options), ownerId_(-1) {

	tilDetonate_ = GetFloatArgument(options, "tilDetonate", 1.5);
	radius_ = GetFloatArgument(options, "radius", 1.0);
	duration_ = GetFloatArgument(options, "duration", 1.0);
	totalDamage_ = GetIntArgument(options, "totalDamage", 10);

	behaviorOptions_["duration"] = GetStringArgument(options, "duration", "1.0");
	behaviorOptions_["totalDamage"] =
		GetStringArgument(options, "totalDamage", "10");
}


bool PoisonGrenadeBehavior::Update(double dt) {
	tilDetonate_ -= dt;
	if (tilDetonate_ > 0) return false;

	// Find all enemies within radius
	vector<GameObject*> objs;
	host_->game()->census()->objsWithin(host_->pos(), radius_, &objs);
	for (uint i = 0; i < objs.size(); i++) {
		GameObject *o = objs[i];
		if (o->enemy() && !o->hasCondition(INVINCIBLE_CONDEFFECT) &&
			!o->hasCondition(STASIS_CONDEFFECT) &&
			!o->hasCondition(INVULNERABLE_CONDEFFECT)) {
			PoisonedBehavior* poisonedBehavior =
				new PoisonedBehavior(o, behaviorOptions_);
			poisonedBehavior->SetOwnerId(ownerId_);
			o->AddBehavior(poisonedBehavior);
		}
	}

	// Nova time
	Message *m = ShowEffectMessage::Nova(host_->id(), radius_, 0xddff00);
	host_->game()->BroadcastIfVisible(m, host_);

	// Destroy host
	host_->Kill(NULL, "Detonated", false, false);

	return true;
}


PoisonedBehavior::PoisonedBehavior(GameObject *host,
	const XMLOptionsMap &options) : Behavior(host, options) {

	timeLeft_ = GetFloatArgument(options, "duration", 1.0);
	damagePerSecond_ = GetIntArgument(options, "totalDamage", 10) / timeLeft_;

	sinceDamageMsg_ = 0;
	recentDamage_ = 0;
}

bool PoisonedBehavior::Update(double dt) {
	timeLeft_ -= dt;
	if (timeLeft_ < 0) {
		SendDamageMessage();
		closed_ = true;
		return false;
	}

	int damage = int(damagePerSecond_ * dt);

	GameObject *shooter = host_->game()->FindObject(ownerId_);
	host_->TakeDamage(damage, shooter, "poison");

	Message *m;
	m = ShowEffectMessage::Poison(host_->id(), 0xddff00);
	host_->game()->BroadcastIfVisible(m, host_);

	sinceDamageMsg_ += dt;
	recentDamage_ += damage;

	if (host_->hitpoints() <= 0 || sinceDamageMsg_ > 1.0) {
		SendDamageMessage();
	}

	return true;
}

void PoisonedBehavior::SendDamageMessage() {
	if (recentDamage_ == 0) return;
	Message *m = new DamageMessage(host_->id(), recentDamage_, host_->dead());
	host_->game()->BroadcastIfVisible(m, host_);

	sinceDamageMsg_ = 0;
	recentDamage_ = 0;
}

ZapBehavior::ZapBehavior(GameObject *host,
	const XMLOptionsMap &options) : Behavior(host, options), poison_(false) {

	radius_ = GetFloatArgument(options, "radius", 5.0);
	damage_ = GetIntArgument(options, "damage", 10);
	cooldown_ = GetFloatArgument(options, "cooldown", 1.0);
	color_ = GetHexIntArgument(options, "color", 0xFFFFFF);
	XMLConditionEffect ce(
		GetStringArgument(options, "conditionEffect", "Nothing").c_str(),
		GetFloatArgument(options, "conditionDuration", 0.0));
	if (ce.effect != NOTHING_CONDEFFECT) {
		effects_.push_back(ce);
	}

	int poisonTotalDamage = GetIntArgument(options, "poisonTotalDamage", 0);
	if (poisonTotalDamage > 0) {
		poison_ = true;
		XMLOptionsMap::const_iterator iter = options.find("poisonDuration");
		poisonBehaviorOptions_["duration"] = iter->second;
		iter = options.find("poisonTotalDamage");
		poisonBehaviorOptions_["totalDamage"] = iter->second;
	}

	cooldownLeft_ = cooldown_;
}

bool ZapBehavior::Update(double dt) {
	cooldownLeft_ -= dt;
	if (cooldownLeft_ > 0) return false;
	cooldownLeft_ = cooldown_;

	// Find closest enemy within radius
	GameObject *closest = NULL;
	double closestDist = radius_ + 1;

	vector<GameObject*> objs;
	host_->game()->census()->objsWithin(host_->pos(), radius_, &objs);
	for (uint i = 0; i < objs.size(); i++) {
		GameObject *o = objs[i];
		double dist = o->DistTo(host_);
		if (o->enemy() && dist < closestDist &&
			!o->hasCondition(INVINCIBLE_CONDEFFECT) &&
			!o->hasCondition(INVULNERABLE_CONDEFFECT) &&
			!o->hasCondition(STASIS_CONDEFFECT) &&
			(effects_.size() == 0 || !o->hasCondition(effects_[0].effect))) {
			closest = o;
			closestDist = dist;
		}
	}

	if (closest == NULL) {
		return false;
	}

	Message *m;
	m = ShowEffectMessage::Line(closest->id(), host_->pos(), color_);
	host_->game()->BroadcastIfVisible(m, host_);
	m = ShowEffectMessage::Nova(closest->id(), 1, color_);
	host_->game()->BroadcastIfVisible(m, closest);
	ushort d = GameObject::damageWithDefense(damage_, closest->defense(),
		false, closest->condition());
	host_->game()->HandleDamage(closest, "Zap", host_, d, &effects_,
		NULL, NULL);
	if (poison_) {
		closest->AddBehavior(new PoisonedBehavior(closest, poisonBehaviorOptions_));
	}

	return true;
}

HealZapBehavior::HealZapBehavior(GameObject *host,
	const XMLOptionsMap &options) : Behavior(host, options) {

	radius_ = GetFloatArgument(options, "radius", 5.0);
	healing_ = GetIntArgument(options, "healing", 25);
	cooldown_ = GetFloatArgument(options, "cooldown", 1.0);
	color_ = GetHexIntArgument(options, "color", 0xFFFFFF);

	cooldownLeft_ = cooldown_;
}

bool HealZapBehavior::Update(double dt) {
	cooldownLeft_ -= dt;
	if (cooldownLeft_ > 0) return false;
	cooldownLeft_ = cooldown_;

	// Find closest enemy within radius
	GameObject *closest = NULL;
	double closestDist = radius_ + 1;

	vector<GameObject*> objs;
	host_->game()->census()->objsWithin(host_->pos(), radius_, &objs);
	for (uint i = 0; i < objs.size(); i++) {
		GameObject *o = objs[i];
		if (!o->isPlayer()) {
			continue;
		}
		double dist = o->DistTo(host_);
		if (dist < closestDist && o->hitpoints() < o->maxHp() &&
			o->hitpoints() > 0) {
			closest = o;
			closestDist = dist;
		}
	}

	if (closest == NULL) {
		return false;
	}

	Message *m;
	m = ShowEffectMessage::Line(closest->id(), host_->pos(), color_);
	host_->game()->BroadcastIfVisible(m, host_);
	closest->Heal(healing_);
	return true;
}

TrapBehavior::TrapBehavior(GameObject *host,
	const XMLOptionsMap &options) : Behavior(host, options), ownerId_(-1) {

	tilArmed_ = GetFloatArgument(options, "tilArmed", 1.5);
	duration_ = GetFloatArgument(options, "duration", 10.0);
	radius_ = GetFloatArgument(options, "radius", 1.0);
	sensitivity_ = GetFloatArgument(options, "sensitivity", 1.0);
	damage_ = GetIntArgument(options, "damage", 10);
	string effectName = GetStringArgument(options, "condEffect", "");
	condEffect_ = ConditionEffect::getCondEffectFromName(effectName.c_str());
	condDuration_ = GetFloatArgument(options, "condDuration", 0.0);
	color_ = GetHexIntArgument(options, "color", 0xaa00ff);

	ticksSincePulse_ = -1;  // So we pulse immediately
}


bool TrapBehavior::Update(double dt) {
	tilArmed_ -= dt;
	if (tilArmed_ > 0) return false;

	if (host_->size() == 0) host_->SetSize(100);

	duration_ -= dt;
	if (duration_ <= 0) {
		BlowUp();
		return true;
	}

	// See if an enemy is within radius * sensitivity

	vector<GameObject*> objs;
	host_->game()->census()->objsWithin(host_->pos(), radius_ * sensitivity_,
		&objs);
	for (uint i = 0; i < objs.size(); i++) {
		GameObject *o = objs[i];
		if (o->enemy() && !o->hasCondition(INVINCIBLE_CONDEFFECT) &&
			!o->hasCondition(STASIS_CONDEFFECT)) {
			BlowUp();
			return true;
		}
	}

	// Maybe show sensitivity radius
	ticksSincePulse_++;
	if (ticksSincePulse_ % 5 == 0) {
		ticksSincePulse_ = 0;
		WorldPosition pos(host_->pos().x, host_->pos().y + radius_);
		Message *m = ShowEffectMessage::Ring(host_->id(), radius_ * sensitivity_,
			color_);
		host_->game()->BroadcastIfVisible(m, host_);
	}

	return true;
}

void TrapBehavior::BlowUp() {
	GameObject *shooter = host_->game()->FindObject(ownerId_);

	vector<GameObject*> objs;
	host_->game()->census()->objsWithin(host_->pos(), radius_, &objs);
	for (uint i = 0; i < objs.size(); i++) {
		GameObject *o = objs[i];
		if (!o->enemy() || o->hasCondition(INVINCIBLE_CONDEFFECT) ||
			o->hasCondition(STASIS_CONDEFFECT)) {
			continue;
		}
		o->AddCondition(condEffect_, condDuration_);
		ushort d = GameObject::damageWithDefense(damage_, o->defense(),
			false, o->condition());
		o->TakeDamage(d, shooter, "trap");
		Message *m = new DamageMessage(o->id(), d, o->dead(), condEffect_);
		host_->game()->BroadcastIfVisible(m, o);
	}

	// Nova time
	Message *m = ShowEffectMessage::Nova(host_->id(), radius_, color_);
	host_->game()->BroadcastIfVisible(m, host_);

	// Destroy host
	host_->Kill(NULL, "Blew up", false, false);
}

TrapTriggerBehavior::TrapTriggerBehavior(GameObject *host,
	const XMLOptionsMap &options) : Behavior(host, options), tilFire_(0.0) {
	radius_ = GetFloatArgument(options, "radius", 1.0);
	radiusVariability_ = GetFloatArgument(options, "radiusVariability", 0.0);
	minDamage_ = GetIntArgument(options, "minDamage", 10);
	maxDamage_ = GetIntArgument(options, "maxDamage", 10);
	posJitter_ = GetFloatArgument(options, "posJitter", 0.0);
	condEffect_ = GetStringArgument(options, "condEffect", "Nothing");
	condDuration_ = GetStringArgument(options, "condDuration", "5.0");
	cooldown_ = GetFloatArgument(options, "cooldown", 3.0);
	cooldownJitter_ = GetFloatArgument(options, "cooldownJitter", 0.0);
	soundId_ = GetIntArgument(options, "soundId", -1);

	string color = GetStringArgument(options, "color", "0xaa00ff");
	Util::Tokenize(color, ",", &colorList_);

	if (colorList_.size() < 1) {
		colorList_.push_back("0xaa00ff");
	}
}

bool TrapTriggerBehavior::Update(double dt) {
	tilFire_ -= dt;

	if (tilFire_ > 0) {
		return false;
	}

	WorldPosition pos = host_->pos();

	if (posJitter_ != 0.0) {
		pos = pos.PlusSalt(posJitter_);
	}

	GameObject *trap = host_->game()->CreateNewObject("Trap", pos);
	XMLOptionsMap behaviorOptions;

	behaviorOptions["tilArmed"] = "0"; // fire immediately
	behaviorOptions["duration"] = "0"; // fire immediately
	behaviorOptions["radius"] = Util::ToString(
		radius_ + Util::PlusMinus(radius_ * radiusVariability_));
	behaviorOptions["damage"] = Util::ToString(
		Util::Random(minDamage_, maxDamage_));
	behaviorOptions["color"] = colorList_[Util::Random(0, colorList_.size() - 1)];
	behaviorOptions["condEffect"] = condEffect_;
	behaviorOptions["condDuration"] = condDuration_;

	TrapBehavior* trapBehavior = new TrapBehavior(trap, behaviorOptions);
	trap->AddBehavior(trapBehavior);

	tilFire_ = cooldown_;

	if (cooldownJitter_ > 0.0) {
		tilFire_ += Util::Random(-cooldownJitter_, cooldownJitter_);
	}

	if (soundId_ >= 0) {
		Message* m = new PlaySoundMessage(host_->id(), soundId_);
		host_->game()->BroadcastIfVisible(m, host_);
	}

	return true;
}

void TrapTriggerBehavior::Clear() {
	tilFire_ = 0.0;
}

ShadowBehavior::ShadowBehavior(GameObject *host,
	const XMLOptionsMap &options)
	: Behavior(host, options), id_(-1), reshadowing_(false) {

	range_ = GetFloatArgument(options, "range", 3.0);
	speed_ = GetFloatArgument(options, "speed", 3.0);
	reshadowRange_ = GetFloatArgument(options, "reshadowRange", 0.5);
}

bool ShadowBehavior::Update(double dt) {
	if (id_ == -1) return false;

	GameObject *target = host_->game()->FindObject(id_);
	if (target == NULL) {
		host_->Expunge();
		return false;
	}
	double dist = host_->DistTo(target);

	double speed = speed_ * dt * 5;

	if (dist <= reshadowRange_ && reshadowing_) {
		reshadowing_ = false;
		return false;
	}

	if (dist > 30) {
		return host_->SetPosition(target->pos(), false);
	}

	if (dist < range_ && !reshadowing_) return false;

	reshadowing_ = true;

	speed *= dist / range_;
	return host_->MoveToward(target->pos(), speed);
}

GrenadeTossBehavior::GrenadeTossBehavior(GameObject *host,
	const XMLOptionsMap &options)
	: Behavior(host, options), shootType_(TARGETED), tilShot_(0.0) {

	string shootTypeStr = GetStringArgument(options, "type", "targeted");
	if (shootTypeStr == "targeted") shootType_ = TARGETED;
	else if (shootTypeStr == "auto") shootType_ = AUTO;
	minRange_ = GetFloatArgument(options, "minRange", 0.0);
	range_ = GetFloatArgument(options, "range", 8.0);
	cooldown_ = GetFloatArgument(options, "cooldown", 1.0);
	cooldownJitter_ = GetBooleanArgument(options, "cooldownJitter", false);
	radius_ = GetFloatArgument(options, "radius", 1.0);
	damage_ = GetIntArgument(options, "damage", 10);
	string effectName = GetStringArgument(options, "effect", "");
	effect_ = effectName.empty() ? 0 :
		ConditionEffect::getCondEffectFromName(effectName.c_str());
	duration_ = GetFloatArgument(options, "duration", 0.0);
	defaultAngle_ = GetFloatArgument(options, "defaultAngle", 90) *
		TO_RADIANS;
}

bool GrenadeTossBehavior::Update(double dt) {
	tilShot_ -= dt;

	if (tilShot_ > 0) return false;

	if (shootType_ == TARGETED) {
		GameObject *target = host_->game()->FindPlayerTargetWithin(host_,
			minRange_, range_);
		double dist = target == NULL ? 1000 : host_->DistTo(target);

		if (dist > range_ + 20) {
			tilShot_ = cooldown_;
			if (cooldownJitter_) {
				tilShot_ += Util::PlusMinus(cooldown_ / 2);
			}
			return false;
		}

		if (dist > range_) return false; // no cooldown, players are pretty close

		if (host_->game()->gameType() == DUNGEON_GAME &&
			!host_->game()->visibility()->CanSee(host_->ipos(), target->ipos())) {
			return false; // no line of sight
		}

		if (host_->hasCondition(STUNNED_CONDEFFECT))
			return false; // stunned

		Toss(target->pos());
	}
	else if (shootType_ == AUTO) {
		WorldPosition pos = host_->pos().PointAt(defaultAngle_, range_);
		if (host_->game()->IsNoWalk(pos) &&
			!host_->game()->NearbyWalkable(&pos)) {
			return false;
		}
		Toss(pos);
	}

	tilShot_ = cooldown_;
	if (cooldownJitter_) {
		tilShot_ += Util::PlusMinus(cooldown_ / 2);
	}
	return true;
}

void GrenadeTossBehavior::Clear() {
	tilShot_ = 0.0;
}

void GrenadeTossBehavior::Toss(const WorldPosition& pos) {
	unordered_set<int> playerIds;
	host_->game()->GetPlayersWhoSee(host_, &playerIds);
	if (playerIds.size() == 0) {
		// Question: If a monster throws a grenade and there are no players there
		// to see it, does he really throw a grenade?
		// Answer: No.
		return;
	}

	GameObject *grenade =
		host_->game()->CreateNewObject("Invisible Placeholder", pos);
	grenade->AddBehavior(new GrenadeBehavior(grenade, 1.5, radius_, damage_,
		effect_, duration_, host_->type(), playerIds));

	Message *m;
	m = ShowEffectMessage::Throw(host_->id(), pos, host_->pos(), 0xff0000);
	host_->game()->BroadcastToPlayers(m, playerIds);
}

GrenadeBehavior::GrenadeBehavior(GameObject *host, float tilDetonate,
	float radius, float damage, uchar effect, float duration, int origType,
	const unordered_set<int>& playerIds) : Behavior(host),
	tilDetonate_(tilDetonate), radius_(radius), damage_(damage),
	effect_(effect), duration_(duration), origType_(origType),
	playerIds_(playerIds) {}

bool GrenadeBehavior::Update(double dt) {
	tilDetonate_ -= dt;
	if (tilDetonate_ > 0) return false;

	Message *m;

	// Send Aoe message
	m = new AoeMessage(host_->pos(), radius_, damage_, effect_, duration_,
		origType_);
	host_->game()->BroadcastToPlayers(m, playerIds_);

	// Destroy host
	host_->Kill(NULL, "Detonated", false, false);

	return true;
}

ObjectTossBehavior::ObjectTossBehavior(GameObject *host,
	const XMLOptionsMap &options)
	: Behavior(host, options), shootType_(TARGETED), tilShot_(0.0) {

	string shootTypeStr = GetStringArgument(options, "type", "targeted");
	if (shootTypeStr == "targeted") shootType_ = TARGETED;
	else if (shootTypeStr == "auto") shootType_ = AUTO;
	childId_ = GetStringArgument(options, "childId", host_->idName());
	minRange_ = GetFloatArgument(options, "minRange", 0.0);
	range_ = GetFloatArgument(options, "range", 8.0);
	rangeJitter_ = GetFloatArgument(options, "rangeJitter", 0.0);
	cooldown_ = GetFloatArgument(options, "cooldown", 1.0);
	cooldownJitter_ = GetBooleanArgument(options, "cooldownJitter", false);
	defaultAngle_ = GetFloatArgument(options, "defaultAngle", 90) *
		TO_RADIANS;
	centerObject_ = GetBooleanArgument(options, "centerObject", false);
}

bool ObjectTossBehavior::Update(double dt) {
	tilShot_ -= dt;

	if (tilShot_ > 0) return false;

	if (shootType_ == TARGETED) {
		GameObject *target = host_->game()->FindPlayerTargetWithin(host_,
			minRange_, range_);
		double dist = target == NULL ? 1000 : host_->DistTo(target);

		if (dist > range_ + 20) {
			tilShot_ = cooldown_;
			if (cooldownJitter_) {
				tilShot_ += Util::PlusMinus(cooldown_ / 2);
			}
			return false;
		}

		if (dist > range_) return false; // no cooldown, players are pretty close

		if (host_->game()->gameType() == DUNGEON_GAME &&
			!host_->game()->visibility()->CanSee(host_->ipos(), target->ipos())) {
			return false; // no line of sight
		}

		if (host_->hasCondition(STUNNED_CONDEFFECT))
			return false; // stunned

		Toss(target->pos());
	}
	else {
		WorldPosition pos = host_->pos().PointAt(defaultAngle_,
			range_ + Util::PlusMinus(rangeJitter_));
		if (centerObject_) {
			pos.x = int(pos.x) + .5;
			pos.y = int(pos.y) + .5;
			if (host_->game()->IsNoWalk(pos)) {
				return false;
			}
		}
		else {
			if (host_->game()->IsNoWalk(pos) &&
				!host_->game()->NearbyWalkable(&pos)) {
				return false;
			}
		}
		Toss(pos);
	}

	tilShot_ = cooldown_;
	if (cooldownJitter_) {
		tilShot_ += Util::PlusMinus(cooldown_ / 2);
	}
	return true;
}

void ObjectTossBehavior::Clear() {
	tilShot_ = 0.0;
}

void ObjectTossBehavior::Toss(const WorldPosition& pos) {
	GameObject *o =
		host_->game()->CreateNewObject("Invisible Placeholder", pos);

	o->AddBehavior(new DecayBehavior(o, 1.5));
	o->AddBehavior(new MetamorphoseAtDeathBehavior(o, childId_, 0.0));

	Message *m;
	m = ShowEffectMessage::Throw(host_->id(), pos, host_->pos(), 0xffc600);
	host_->game()->BroadcastIfVisible(m, host_);
}


LootDuplicationBehavior::LootDuplicationBehavior(GameObject *host,
	const XMLOptionsMap &options) : Behavior(host, options) {
	tilStopDuplicating_ = 10.0;

	host_->inventory()->SetDuplicating(true);
}

bool LootDuplicationBehavior::Update(double dt) {
	tilStopDuplicating_ -= dt;

	Message *m;
	m = ShowEffectMessage::Poison(host_->id(), 0xEDDA09);
	host_->game()->BroadcastIfVisible(m, host_);

	if (tilStopDuplicating_ > 0) return false;

	host_->inventory()->SetDuplicating(false);
	closed_ = true;
	return true;
}



WalkBehavior::WalkBehavior(GameObject *host,
	const XMLOptionsMap &options) : Behavior(host, options) {
	timeLeft_ = 0;

	speed_ = GetFloatArgument(options, "speed", 0.4);
	string groundTypes = GetStringArgument(options, "groundTypes", "");
	if (!groundTypes.empty()) {
		vector<string> groundTypeList;
		Util::Tokenize(groundTypes, ",", &groundTypeList);
		for (uint i = 0; i < groundTypeList.size(); i++) {
			groundTypes_.insert(Game::GroundTypeFromId(groundTypeList[i]));
		}
	}
}

void WalkBehavior::Clear() {
	timeLeft_ = 0;
}

bool WalkBehavior::Update(double dt) {
	if (timeLeft_ > 0) {
		timeLeft_ -= dt;
		if (!host_->MoveToward(targetPos_, speed_ * dt * 5, false)) {
			timeLeft_ = 0;
		}
		return true;
	}

	float angle = Util::Random(0.0, 2.0 * PI);
	float dist = Util::Random(3, 6);
	WorldPosition pos(host_->x() + dist * cos(angle),
		host_->y() + dist * sin(angle));
	if (pos.OutOfBounds(host_->game()->worldWidth())) {
		return true;
	}
	if (groundTypes_.size() > 0) {
		const Tile& tile = host_->game()->Map(pos);
		if (groundTypes_.find(tile.ground) == groundTypes_.end()) {
			return true;
		}
	}

	targetPos_ = pos;
	timeLeft_ = 10.0 + Util::Random(0.0, 10.0);

	return true;
}

StatBoostedBehavior::StatBoostedBehavior(
	GameObject *host, int stat, int amount, double duration) :
	Behavior(host), stat_(stat), amount_(amount), timeLeft_(duration) {

	host_->IncrementStatBonus(stat_, amount_);
	if (stat_ == MAX_HP_STAT) host_->IncrementStat(HP_STAT, amount_);
	if (stat_ == MAX_MP_STAT) host_->IncrementStat(MP_STAT, amount_);
}

bool StatBoostedBehavior::Update(double dt) {
	timeLeft_ -= dt;
	if (timeLeft_ < 0) {
		host_->IncrementStatBonus(stat_, -amount_);
		closed_ = true;
		return false;
	}

	return false;
}

ShowEffectBehavior::ShowEffectBehavior(GameObject* host,
	const XMLOptionsMap &options) : Behavior(host) {

	effectType_ = ShowEffectMessage::TypeNameToType(
		GetStringArgument(options, "type", "Flash").c_str());
	color_ = GetHexIntArgument(options, "color", 0xFFFFFF);
	flashPeriod_ = GetFloatArgument(options, "flashPeriod", 0.75);
	flashRepeats_ = GetFloatArgument(options, "flashRepeats", 1.0);

	Clear();
}

bool ShowEffectBehavior::Update(double dt) {
	if (sent_) {
		return true;
	}

	ShowEffectMessage* m =
		new ShowEffectMessage(effectType_, host_->id(), color_);
	if (effectType_ == ShowEffectMessage::FLASH_EFFECT_TYPE) {
		m->setPos1(flashPeriod_, flashRepeats_);
	}
	host_->game()->BroadcastIfVisible(m, host_);
	sent_ = true;
	return true;
}

void ShowEffectBehavior::Clear() {
	sent_ = false;
}

ConditionEffectBehavior::ConditionEffectBehavior(GameObject* host,
	const XMLOptionsMap &options) : Behavior(host, options), started_(false) {
	string effectName = GetStringArgument(options, "effect", "");
	effect_ = ConditionEffect::getCondEffectFromName(effectName.c_str());
	duration_ = GetFloatArgument(options, "duration", 0.0);
}

bool ConditionEffectBehavior::Update(double dt) {
	if (started_) {
		return false;
	}
	started_ = true;

	if (duration_ <= 0.0) {
		host_->AddPermanentCondition(effect_);
	}
	else {
		host_->AddCondition(effect_, duration_);
	}
	return true;
}

void ConditionEffectBehavior::Clear() {
	host_->RemoveCondition(effect_);
	started_ = false;
}

SetAltTextureBehavior::SetAltTextureBehavior(GameObject* host,
	const XMLOptionsMap &options) : Behavior(host, options), started_(false) {
	altTextureId_ = GetIntArgument(options, "altTextureId", 0);
}

bool SetAltTextureBehavior::Update(double dt) {
	if (started_) {
		return false;
	}
	started_ = true;

	host_->SetAltTexture(altTextureId_);
	return true;
}

void SetAltTextureBehavior::Clear() {
	host_->SetAltTexture(0);
	started_ = false;
}

SetTagBehavior::SetTagBehavior(GameObject* host,
	const XMLOptionsMap &options) : Behavior(host, options) {
	tagName_ = GetStringArgument(options, "tagName", "");
}

bool SetTagBehavior::Update(double dt) {
	host_->setTag(tagName_);
	return true;
}

ClearTagBehavior::ClearTagBehavior(GameObject* host,
	const XMLOptionsMap &options) : Behavior(host, options) {
	tagName_ = GetStringArgument(options, "tagName", "");
}

bool ClearTagBehavior::Update(double dt) {
	host_->clearTag(tagName_);
	return true;
}

SetGlobalTagBehavior::SetGlobalTagBehavior(GameObject* host,
	const XMLOptionsMap &options) : Behavior(host, options),
	notificationCooldown_(3.0), notificationCooldownLeft_(0.0),
	notificationType_(0)  {
	tagName_ = GetStringArgument(options, "tagName", "");
	sendNotification_ = GetBooleanArgument(options, "sendNotification", false);

	if (sendNotification_) {
		notificationCooldown_ = GetFloatArgument(options, "notificationCooldown",
			3.0);
		notificationType_ = GetIntArgument(options, "notificationType", 0);
	}
}

bool SetGlobalTagBehavior::Update(double dt) {
	host_->game()->setTag(tagName_);

	if (!sendNotification_) {
		return true;
	}

	notificationCooldownLeft_ -= dt;

	// reduce traffic for long running notification
	if (notificationCooldownLeft_ > 0) {
		return true;
	}

	Message *m = new GlobalNotificationMessage(notificationType_, tagName_);

	host_->game()->Broadcast(m, NULL);
	host_->game()->AddInitialNotification(tagName_);
	notificationCooldownLeft_ = notificationCooldown_;

	return true;
}

void SetGlobalTagBehavior::Clear() {
	notificationCooldownLeft_ = 0.0;
}

ClearGlobalTagBehavior::ClearGlobalTagBehavior(GameObject* host,
	const XMLOptionsMap &options) : Behavior(host, options) {
	tagName_ = GetStringArgument(options, "tagName", "");
}

bool ClearGlobalTagBehavior::Update(double dt) {
	host_->game()->clearTag(tagName_);
	return true;
}

RestoresBreathBehavior::RestoresBreathBehavior(GameObject* host,
	const XMLOptionsMap &options) : Behavior(host, options) {
	radius_ = GetFloatArgument(options, "radius", 1.0);
	radiusSq_ = radius_ * radius_;
	speed_ = GetFloatArgument(options, "speed", 50.0);
}

bool RestoresBreathBehavior::Update(double dt) {
	vector<GameObject*> objs;
	host_->game()->census()->objsWithin(host_->pos(), radius_, &objs);
	for (uint i = 0; i < objs.size(); i++) {
		GameObject *o = objs[i];
		if (!o->isPlayer()) {
			continue;
		}
		double distSq = o->SqDistTo(host_);
		if (distSq < radiusSq_) {
			o->SetBreath(MIN(100.0, o->breath() + dt * speed_));
		}
	}
	return true;
}

CopyDamageRecordBehavior::CopyDamageRecordBehavior(GameObject* host,
	const XMLOptionsMap &options) : Behavior(host, options),
	sinceCopy_(0.0), targetId_(-1) {
	targetType_ = Game::ObjectTypeFromId(
		GetStringArgument(options, "targetId", ""));
	range_ = GetFloatArgument(options, "range", 40.0);
}

bool CopyDamageRecordBehavior::Update(double dt) {
	sinceCopy_ += dt;
	if (sinceCopy_ < 1.0) {
		return false;
	}
	sinceCopy_ = 0.0;

	if (targetId_ < 0) {
		targetId_ = findClosest(range_, targetType_);
	}

	GameObject* target = host_->game()->FindObject(targetId_);
	if (target == NULL) {
		targetId_ = -1;
		return false;
	}

	if (host_->damageRecord()->total() == target->damageRecord()->total()) {
		return true;
	}
	host_->damageRecord()->copy(*target->damageRecord());
	return true;
}

ChangeGroundBehavior::ChangeGroundBehavior(GameObject* host,
	const XMLOptionsMap &options) : Behavior(host, options) {
	groundType_ = Game::GroundTypeFromId(
		GetStringArgument(options, "groundId", ""));
}

bool ChangeGroundBehavior::Update(double dt) {
	Tile* tile = host_->game()->getTile(host_->x(), host_->y());
	if (tile->ground == groundType_) {
		return false;
	}
	tile->ground = groundType_;
	host_->game()->InvalidateTile(host_->x(), host_->y());
	return true;
}

PlaySoundBehavior::PlaySoundBehavior(GameObject* host,
	const XMLOptionsMap &options) : Behavior(host, options), played_(false) {
	soundId_ = GetIntArgument(options, "soundId", 0);
}

bool PlaySoundBehavior::Update(double dt) {
	if (played_) {
		return false;
	}

	Message* m = new PlaySoundMessage(host_->id(), soundId_);
	host_->game()->BroadcastIfVisible(m, host_);
	played_ = true;
	return true;
}

void PlaySoundBehavior::Clear() {
	played_ = false;
}

PlaceMapBehavior::PlaceMapBehavior(GameObject* host,
	const XMLOptionsMap &options) : Behavior(host, options), cooldownLeft_(0) {
	// to-do: check file name
	mapFile_ = DATA_JM_BASE + GetStringArgument(options, "map", "");
	// placeLeft_ = -1 means infinite
	placeLeft_ = GetIntArgument(options, "maxPlace", 1);

	// normalize cooldown_, we will use cooldown_ = 0.0 to indicate the behavior
	// will be run only once
	if (placeLeft_ > 1 || placeLeft_ < 0) {
		cooldown_ = GetFloatArgument(options, "cooldown", 5.0);
	}
	else {
		cooldown_ = 0.0;
	}

	if (cooldown_ <= 0) {
		placeLeft_ = 1;
		cooldown_ = 0.0;
	}
}

bool PlaceMapBehavior::Update(double dt) {
	if (placeLeft_ == 0) {
		closed_ = true;
		return false;
	}

	// only multi places needs cooldown
	if (cooldown_ != 0.0) {
		if (cooldownLeft_ > 0) {
			cooldownLeft_ -= dt;
			return false;
		}

		cooldownLeft_ = cooldown_;
	}

	// get map from map cache
	const ParsedMap* setting = ParsedMap::ParseFromFile(mapFile_.c_str());

	if (setting == NULL) {
		closed_ = true;
		return false;
	}

	// place the map at object's current locatino
	setting->addToMapCenter(host_->game(), host_->pos().x, host_->pos().y, 0,
		NULL);

	// if placeLeft_ = -1 (infinite places), don't bother it
	if (placeLeft_ > 0) {
		--placeLeft_;
	}

	return true;
}

void PlaceMapBehavior::Clear() {
	cooldownLeft_ = 0.0;
}

DeleteObjectBehavior::DeleteObjectBehavior(GameObject* host,
	const XMLOptionsMap &options) : Behavior(host, options), isDeleted_(false),
	objectTypes_(NULL), radius_(0.0) {
	string objectIdsString = GetStringArgument(options, "objectId", "");
	vector<string> objectIds;

	Util::Tokenize(objectIdsString, ",", &objectIds);
	uint count = objectIds.size();

	if (count > 0) {
		objectTypes_ = new unordered_set<int>;
		radius_ = GetFloatArgument(options, "radius", 0.0);
	}

	for (uint i = 0; i < count; ++i) {
		int type = host_->game()->ObjectTypeFromId(objectIds[i]);

		if (type == -1) {
			Util::Log() << "Cannot find object id " << objectIds[i] << endl;
			exit(1);
		}

		objectTypes_->insert(type);
	}
}

DeleteObjectBehavior::~DeleteObjectBehavior() {
	delete objectTypes_;
}

bool DeleteObjectBehavior::Update(double dt) {
	if (isDeleted_) {
		return false;
	}

	isDeleted_ = true;

	if ((objectTypes_ == NULL) || (radius_ == 0.0)) {
		return false;
	}

	vector<GameObject*> objs;

	host_->game()->census()->objsWithin(host_->pos(), radius_, &objs);
	host_->game()->FindStaticObjectsWithin(host_->pos(), radius_, &objs);

	vector<IntPoint> tiles;
	unordered_set<int>::const_iterator notFound = objectTypes_->end();

	for (uint i = 0; i < objs.size(); i++) {
		GameObject *o = objs[i];

		if (objectTypes_->find(o->type()) == notFound) {
			continue;
		}

		o->Expunge();

		if (o->isStatic()) {
			tiles.push_back(IntPoint(o->x(), o->y()));
		}
	}

	host_->game()->InvalidateTiles(tiles);

	return true;
}

void DeleteObjectBehavior::Clear() {
	isDeleted_ = false;
}
